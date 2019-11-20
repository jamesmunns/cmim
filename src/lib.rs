#![no_std]

#[macro_export]
macro_rules! cmim {
    (
        $($name:ident: $ty:ty => $intr:expr,)+
    ) => {
        pub(crate) mod cmim_inner {
            use super::Interrupt;

            pub(crate) struct CmimInnerData<T> {
                data: ::core::mem::MaybeUninit<T>,
                inter: Interrupt,
            }

            impl<T> CmimInnerData<T> {
                pub(crate) unsafe fn unsafe_get(&mut self) -> &mut T {
                    &mut *self.data.as_mut_ptr()
                }

                pub(crate) unsafe fn unsafe_set(&mut self, input: T) {
                    self.data.as_mut_ptr().write(input);
                }

                pub(crate) unsafe fn unsafe_get_inter(&self) -> u8 {
                    ::bare_metal::Nr::nr(&self.inter)
                }
            }

            $(
                pub(crate) static mut $name: CmimInnerData<$ty> = CmimInnerData {
                    data: ::core::mem::MaybeUninit::uninit(),
                    inter: $intr
                };
            )+
        }
    }
}

#[macro_export]
macro_rules! cmim_set {
    ($name:ident = $val:expr) => {{
        // If the interrupt is enabled, return
        let enabled = {
            // Note: This is a copy of `NVIC::is_enabled()`, which sadly takes
            // ownership rather than references
            let nr = unsafe { crate::cmim_inner::$name.unsafe_get_inter() };
            let mask = 1 << (nr % 32);

            // NOTE(unsafe) atomic read with no side effects
            unsafe {
                ((*::cortex_m::peripheral::NVIC::ptr()).ispr[usize::from(nr / 32)].read() & mask)
                    == mask
            }
        };

        if enabled {
            Err(())
        } else {
            unsafe {
                crate::cmim_inner::$name.unsafe_set($val);
                Ok(())
            }
        }
    }};
}

/// This macro is dangerous for multiple reasons:
/// * It has no re-entrancy check, so you could totally get multiple
///   mutable references in scope at the same time if you use it more than once
/// * It has no check to see if you've actually ever set the data, which means
//    that you could totally get uninitialized memory
#[macro_export]
macro_rules! cmim_get {
    ($name: ident) => {
        if let ::cortex_m::peripheral::scb::VectActive::Interrupt { irqn } =
            ::cortex_m::peripheral::SCB::vect_active()
        {
            if irqn == unsafe { crate::cmim_inner::$name.unsafe_get_inter() } {
                Ok(unsafe { crate::cmim_inner::$name.unsafe_get() })
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    };
}

#[cfg(feature = "nope")]
mod test {

    #[derive(Copy, Clone)]
    pub enum Interrupt {
        RADIO,
        VIDEO,
        AUDIO,
    }

    unsafe impl Nr for Interrupt {
        fn nr(&self) -> u8 {
            return 0;
        }
    }

    cmim! {
        FOO: bool => Interrupt::RADIO,
        BAR: u32  => Interrupt::VIDEO,
        BAZ: u64  => Interrupt::AUDIO,
    }

    use cortex_m::interrupt::Nr;
    use cortex_m::peripheral::scb::VectActive;
    use cortex_m::peripheral::{NVIC, SCB};

    fn main() {
        cmim_set!(BAZ, 64u64).unwrap();
    }

    fn interrupt() {
        let x = cmim_get!(BAZ).unwrap();
    }
}
