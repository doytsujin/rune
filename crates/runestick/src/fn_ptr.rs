use crate::context::Handler;
use crate::unit::UnitFnCall;
use crate::VmErrorKind;
use crate::{
    CallVm, Context, FromValue, Future, Generator, Hash, IntoArgs, Shared, Stack, StopReason,
    Tuple, Unit, Value, Vm, VmError,
};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

/// A stored function, of some specific kind.
#[derive(Debug)]
pub struct FnPtr {
    inner: Inner,
}

impl FnPtr {
    /// Perform a call over the function represented by this function pointer.
    pub fn call<A, T>(&self, args: A) -> Result<T, VmError>
    where
        A: IntoArgs,
        T: FromValue,
    {
        let value = match &self.inner {
            Inner::FnHandler(handler) => {
                let mut stack = Stack::with_capacity(A::count());
                args.into_args(&mut stack)?;
                (handler.handler)(&mut stack, A::count())?;
                stack.pop()?
            }
            Inner::FnPtrOffset(offset) => {
                Self::check_args(A::count(), offset.args)?;

                let mut vm = Vm::new(offset.context.clone(), offset.unit.clone());
                vm.set_ip(offset.offset);
                args.into_args(vm.stack_mut())?;

                match offset.call {
                    UnitFnCall::Generator => Value::Generator(Shared::new(Generator::new(vm))),
                    UnitFnCall::Immediate => vm.complete()?,
                    UnitFnCall::Async => Value::Future(Shared::new(Future::new(async move {
                        vm.async_complete().await
                    }))),
                }
            }
            Inner::FnClosureOffset(offset) => {
                Self::check_args(A::count(), offset.args)?;

                let mut vm = Vm::new(offset.context.clone(), offset.unit.clone());
                vm.set_ip(offset.offset);
                args.into_args(vm.stack_mut())?;
                vm.stack_mut()
                    .push(Value::Tuple(offset.environment.clone()));

                match offset.call {
                    UnitFnCall::Generator => Value::Generator(Shared::new(Generator::new(vm))),
                    UnitFnCall::Immediate => vm.complete()?,
                    UnitFnCall::Async => {
                        let future = Future::new(async move { vm.async_complete().await });
                        Value::Future(Shared::new(future))
                    }
                }
            }
            Inner::FnTuple(tuple) => {
                Self::check_args(A::count(), tuple.args)?;
                Value::typed_tuple(tuple.hash, args.into_vec()?)
            }
            Inner::FnVariantTuple(tuple) => {
                Self::check_args(A::count(), tuple.args)?;
                Value::variant_tuple(tuple.enum_hash, tuple.hash, args.into_vec()?)
            }
        };

        Ok(T::from_value(value)?)
    }

    /// Create a function pointer from a handler.
    pub(crate) fn from_handler(handler: Arc<Handler>) -> Self {
        Self {
            inner: Inner::FnHandler(FnHandler { handler }),
        }
    }

    /// Create a function pointer from an offset.
    pub(crate) fn from_offset(
        context: Rc<Context>,
        unit: Rc<Unit>,
        offset: usize,
        call: UnitFnCall,
        args: usize,
    ) -> Self {
        Self {
            inner: Inner::FnPtrOffset(FnPtrOffset {
                context,
                unit,
                offset,
                call,
                args,
            }),
        }
    }

    /// Create a function pointer from an offset.
    pub(crate) fn from_closure(
        context: Rc<Context>,
        unit: Rc<Unit>,
        environment: Shared<Tuple>,
        offset: usize,
        call: UnitFnCall,
        args: usize,
    ) -> Self {
        Self {
            inner: Inner::FnClosureOffset(FnClosureOffset {
                context,
                unit,
                environment,
                offset,
                call,
                args,
            }),
        }
    }

    /// Create a function pointer from an offset.
    pub(crate) fn from_tuple(hash: Hash, args: usize) -> Self {
        Self {
            inner: Inner::FnTuple(FnTuple { hash, args }),
        }
    }

    /// Create a function pointer that constructs a tuple variant.
    pub(crate) fn from_variant_tuple(enum_hash: Hash, hash: Hash, args: usize) -> Self {
        Self {
            inner: Inner::FnVariantTuple(FnVariantTuple {
                enum_hash,
                hash,
                args,
            }),
        }
    }

    /// Call with the given virtual machine. This allows for certain
    /// optimizations, like avoiding the allocation of a new vm state in case
    /// the call is internal.
    ///
    /// A stop reason will be returned in case the function call results in
    /// a need to suspend the execution.
    pub(crate) fn call_with_vm(
        &self,
        vm: &mut Vm,
        args: usize,
    ) -> Result<Option<StopReason>, VmError> {
        let reason = match &self.inner {
            Inner::FnHandler(handler) => {
                (handler.handler)(vm.stack_mut(), args)?;
                None
            }
            Inner::FnPtrOffset(offset) => {
                Self::check_args(args, offset.args)?;

                // Fast past, just allocate a call frame and keep running.
                if let UnitFnCall::Immediate = offset.call {
                    if vm.is_same(&offset.context, &offset.unit) {
                        vm.push_call_frame(offset.offset, args)?;
                        return Ok(None);
                    }
                }

                let new_stack = vm.stack_mut().drain_stack_top(args)?.collect::<Stack>();
                let mut vm =
                    Vm::new_with_stack(offset.context.clone(), offset.unit.clone(), new_stack);
                vm.set_ip(offset.offset);
                Some(StopReason::CallVm(CallVm::new(offset.call, vm)))
            }
            Inner::FnClosureOffset(offset) => {
                Self::check_args(args, offset.args)?;

                // Fast past, just allocate a call frame, push the environment
                // onto the stack and keep running.
                if let UnitFnCall::Immediate = offset.call {
                    if vm.is_same(&offset.context, &offset.unit) {
                        vm.push_call_frame(offset.offset, args)?;
                        vm.stack_mut()
                            .push(Value::Tuple(offset.environment.clone()));
                        return Ok(None);
                    }
                }

                let mut new_stack = Stack::new();
                new_stack.extend(vm.stack_mut().drain_stack_top(args)?);
                new_stack.push(Value::Tuple(offset.environment.clone()));
                let mut vm =
                    Vm::new_with_stack(offset.context.clone(), offset.unit.clone(), new_stack);
                vm.set_ip(offset.offset);
                Some(StopReason::CallVm(CallVm::new(offset.call, vm)))
            }
            Inner::FnTuple(tuple) => {
                Self::check_args(args, tuple.args)?;
                let value = Value::typed_tuple(tuple.hash, vm.stack_mut().pop_sequence(args)?);
                vm.stack_mut().push(value);
                None
            }
            Inner::FnVariantTuple(tuple) => {
                Self::check_args(args, tuple.args)?;

                let value = Value::variant_tuple(
                    tuple.enum_hash,
                    tuple.hash,
                    vm.stack_mut().pop_sequence(args)?,
                );

                vm.stack_mut().push(value);
                None
            }
        };

        Ok(reason)
    }

    #[inline]
    fn check_args(actual: usize, expected: usize) -> Result<(), VmError> {
        if actual != expected {
            return Err(VmError::from(VmErrorKind::ArgumentCountMismatch {
                expected,
                actual,
            }));
        }

        Ok(())
    }
}

#[derive(Debug)]
enum Inner {
    FnHandler(FnHandler),
    FnPtrOffset(FnPtrOffset),
    FnTuple(FnTuple),
    FnClosureOffset(FnClosureOffset),
    FnVariantTuple(FnVariantTuple),
}

struct FnHandler {
    /// The function handler.
    handler: Arc<Handler>,
}

impl fmt::Debug for FnHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FnHandler")
    }
}

struct FnPtrOffset {
    context: Rc<Context>,
    /// The unit where the function resides.
    unit: Rc<Unit>,
    /// The offset of the function.
    offset: usize,
    /// The calling convention.
    call: UnitFnCall,
    /// The number of arguments the function takes.
    args: usize,
}

impl fmt::Debug for FnPtrOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FnPtrOffset")
            .field("context", &(&self.context as *const _))
            .field("unit", &(&self.unit as *const _))
            .field("offset", &self.offset)
            .field("call", &self.call)
            .field("args", &self.args)
            .finish()
    }
}

struct FnClosureOffset {
    context: Rc<Context>,
    /// The unit where the function resides.
    unit: Rc<Unit>,
    /// Captured environment.
    environment: Shared<Tuple>,
    /// The offset of the function.
    offset: usize,
    /// The calling convention.
    call: UnitFnCall,
    /// The number of arguments the function takes.
    args: usize,
}

impl fmt::Debug for FnClosureOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FnPtrOffset")
            .field("context", &(&self.context as *const _))
            .field("unit", &(&self.unit as *const _))
            .field("environment", &self.environment)
            .field("offset", &self.offset)
            .field("call", &self.call)
            .field("args", &self.args)
            .finish()
    }
}

#[derive(Debug)]
struct FnTuple {
    /// The type of the tuple.
    hash: Hash,
    /// The number of arguments the tuple takes.
    args: usize,
}

#[derive(Debug)]
struct FnVariantTuple {
    /// The enum the variant belongs to.
    enum_hash: Hash,
    /// The type of the tuple.
    hash: Hash,
    /// The number of arguments the tuple takes.
    args: usize,
}