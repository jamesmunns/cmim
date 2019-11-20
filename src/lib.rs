#![no_std]

#[macro_export]
macro_rules! cmim {
    (
        $($name:ident: $ty:ty => $intr:expr,)+
    ) => {
        struct Inner<T> {
            data: ::core::mem::MaybeUninit<T>,
            inter: Interrupt,
        }

        impl<T> Inner<T> {
            pub(crate) unsafe fn unsafe_get(&mut self) -> &mut T {
                &mut *self.data.as_mut_ptr()
            }

            pub(crate) unsafe fn unsafe_set(&mut self, input: T) {
                self.data.as_mut_ptr().write(input);
            }

            pub(crate) unsafe fn unsafe_get_inter(&self) -> u8 {
                use ::bare_metal::Nr;
                self.inter.nr()
            }
        }

        $(
            static mut $name: Inner<$ty> = Inner {
                data: ::core::mem::MaybeUninit::uninit(),
                inter: $intr
            };
        )+
    }
}

#[macro_export]
macro_rules! cmim_set {
    ($name:ident = $val:expr) => {{
        // If the interrupt is enabled, return
        let enabled = {
            // Note: This is a copy of `NVIC::is_enabled()`, which sadly takes
            // ownership rather than references
            let nr = unsafe { $name.unsafe_get_inter() };
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
                $name.unsafe_set($val);
                Ok(())
            }
        }
    }};
}

#[macro_export]
macro_rules! cmim_get {
    ($name: ident) => {
        if let ::cortex_m::peripheral::scb::VectActive::Interrupt { irqn } =
            ::cortex_m::peripheral::SCB::vect_active()
        {
            if irqn == unsafe { $name.unsafe_get_inter() } {
                Ok(unsafe { $name.unsafe_get() })
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
