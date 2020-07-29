/// Implement the value trait for an external type.
///
/// This is required to support the external type as a type argument in a
/// registered function.
///
/// This will be **deprecated** once (or if) [specialization] lands.
///
/// [specialization]: https://github.com/rust-lang/rust/issues/31844
#[macro_export]
macro_rules! decl_external {
    ($external:ty) => {
        impl $crate::ReflectValueType for $external {
            fn reflect_value_type() -> $crate::ValueType {
                $crate::ValueType::External(std::any::TypeId::of::<$external>())
            }
        }

        impl<'a> $crate::ReflectValueType for &'a $external {
            fn reflect_value_type() -> $crate::ValueType {
                $crate::ValueType::External(std::any::TypeId::of::<$external>())
            }
        }

        impl<'a> $crate::ReflectValueType for &'a mut $external {
            fn reflect_value_type() -> $crate::ValueType {
                $crate::ValueType::External(std::any::TypeId::of::<$external>())
            }
        }

        impl $crate::ToValue for $external {
            fn to_value(self, vm: &mut $crate::Vm) -> Option<$crate::ValueRef> {
                Some(vm.allocate_external(self))
            }
        }

        impl $crate::TakeValue for $external {
            fn take_value(
                value: $crate::ValueRef,
                vm: &mut $crate::Vm,
            ) -> Result<Self, $crate::ValueRef> {
                let slot = value.into_external()?;

                match vm.external_take::<$external>(slot) {
                    Some(external) => Ok(external),
                    None => Err(value),
                }
            }
        }

        impl<'a> $crate::UnsafeFromValue for $external {
            unsafe fn unsafe_from_value(
                value: $crate::ValueRef,
                vm: &mut $crate::Vm,
            ) -> Result<Self, $crate::ValueRef> {
                let slot = value.into_external()?;

                match vm.external_take::<$external>(slot) {
                    Some(value) => Ok(value),
                    None => Err(value),
                }
            }
        }

        impl<'a> $crate::UnsafeFromValue for &'a $external {
            unsafe fn unsafe_from_value(
                value: $crate::ValueRef,
                vm: &mut $crate::Vm,
            ) -> Result<Self, $crate::ValueRef> {
                let slot = value.into_external()?;

                match vm.external_ref::<$external>(slot) {
                    Some(value) => Ok(value),
                    None => Err(value),
                }
            }
        }

        impl<'a> $crate::UnsafeFromValue for &'a mut $external {
            unsafe fn unsafe_from_value(
                value: $crate::ValueRef,
                vm: &mut $crate::Vm,
            ) -> Result<Self, $crate::ValueRef> {
                let slot = value.into_external()?;

                match vm.external_mut::<$external>(slot) {
                    Some(value) => Ok(value),
                    None => Err(value),
                }
            }
        }
    };
}
