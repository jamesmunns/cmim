//! # Cortex-M Interrupt Move
//!
//! It's the next best thing to moving to interrupt context.
//!
//! ## Examples
//!
//! Check out the full examples in the [`app-examples`](./app-examples) folder.
//!
//! ## The goal
//!
//! The goal here is to replace usage of a mutex which may require an entire critical section, and instead model "Moving" of data to an interrupt context.
//!
//! This means that we don't need a critical section to access it, we just need to be in the interrupt we moved the data to.
//!
//! Here's how it works:
//!
//! ```rust, no_run
//! #![no_main]
//!
//! // CMIM items
//! use cmim::{
//!     Move,
//!     Context,
//!     Exception,
//! };
//!
//! // Used to set the program entry point
//! use cortex_m_rt::{entry, exception};
//!
//! use embedded_hal::timer::Cancel;
//!
//! // Provides definitions for our development board
//! use dwm1001::{
//!     cortex_m::peripheral::syst::SystClkSource::Core,
//!     nrf52832_hal::{
//!         nrf52832_pac::{interrupt, Interrupt, TIMER1, UARTE0},
//!         prelude::*,
//!         Timer, Uarte,
//!     },
//!     DWM1001,
//! };
//!
//! static TIMER_1_DATA: Move<Timer1Data, Interrupt> = Move::new_uninitialized(Context::Interrupt(Interrupt::TIMER1));
//! static SYSTICK_DATA: Move<SysTickData, Interrupt> = Move::new_uninitialized(Context::Exception(Exception::SysTick));
//!
//! struct Timer1Data {
//!     uart: Uarte<UARTE0>,
//!     timer: Timer<TIMER1>,
//!     led: dwm1001::Led,
//!     toggle: bool,
//! }
//!
//! struct SysTickData {
//!     led: dwm1001::Led,
//!     toggle: bool,
//! }
//!
//! #[entry]
//! fn main() -> ! {
//!     if let Some(mut board) = DWM1001::take() {
//!         let mut timer = board.TIMER0.constrain();
//!         let mut _rng = board.RNG.constrain();
//!
//!         let mut itimer = board.TIMER1.constrain();
//!         itimer.start(1_000_000u32);
//!         itimer.enable_interrupt(&mut board.NVIC);
//!
//!         // Core clock is 64MHz, blink at 16Hz
//!         board.SYST.set_clock_source(Core);
//!         board.SYST.set_reload(4_000_000 - 1);
//!         board.SYST.enable_counter();
//!         board.SYST.enable_interrupt();
//!
//!         TIMER_1_DATA
//!             .try_move(Timer1Data {
//!                 uart: board.uart,
//!                 timer: itimer,
//!                 led: board.leds.D9,
//!                 toggle: false,
//!             })
//!             .ok();
//!
//!         SYSTICK_DATA
//!             .try_move(SysTickData {
//!                 led: board.leds.D12,
//!                 toggle: false,
//!             })
//!             .ok();
//!
//!         let mut toggle = false;
//!
//!         loop {
//!             // board.leds.D9  - Top LED GREEN
//!             // board.leds.D12 - Top LED RED
//!             // board.leds.D11 - Bottom LED RED
//!             // board.leds.D10 - Bottom LED BLUE
//!             if toggle {
//!                 board.leds.D10.enable();
//!             } else {
//!                 board.leds.D10.disable();
//!             }
//!
//!             toggle = !toggle;
//!
//!             timer.delay(250_000);
//!         }
//!     }
//!
//!     loop {
//!         continue;
//!     }
//! }
//!
//! #[exception]
//! fn SysTick() {
//!     SYSTICK_DATA
//!         .try_lock(|data| {
//!             // Blink the LED
//!             if data.toggle {
//!                 data.led.enable();
//!             } else {
//!                 data.led.disable();
//!             }
//!
//!             data.toggle = !data.toggle;
//!         })
//!         .ok();
//! }
//!
//! #[interrupt]
//! fn TIMER1() {
//!     TIMER_1_DATA
//!         .try_lock(|data| {
//!             // Start the timer again first for accuracy
//!             data.timer.cancel().unwrap();
//!             data.timer.start(1_000_000u32);
//!
//!             // Write message to UART. The NRF UART requires data
//!             // to be in RAM, not flash.
//!             const MSG_BYTES: &[u8] = "Blink!\r\n".as_bytes();
//!             let mut buf = [0u8; MSG_BYTES.len()];
//!             buf.copy_from_slice(MSG_BYTES);
//!
//!             data.uart.write(&buf).unwrap();
//!
//!             // Blink the LED
//!             if data.toggle {
//!                 data.led.enable();
//!             } else {
//!                 data.led.disable();
//!             }
//!
//!             data.toggle = !data.toggle;
//!         })
//!         .ok();
//! }
//! ```
//!
//!
//! # License
//!
//! Licensed under either of
//!
//! - Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
//!   http://www.apache.org/licenses/LICENSE-2.0)
//!
//! - MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
//!
//! at your option.
//!
//! ## Contribution
//!
//! Unless you explicitly state otherwise, any contribution intentionally submitted
//! for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
//! dual licensed as above, without any additional terms or conditions.

#![no_std]

use core::{
    cell::UnsafeCell,
    cmp::PartialEq,
    mem::MaybeUninit,
    result::Result,
    sync::atomic::{AtomicU8, Ordering},
};

use bare_metal::Nr;
use cortex_m::interrupt::free;
use cortex_m::peripheral::{scb::VectActive, SCB};
pub use cortex_m::peripheral::scb::Exception;

/// Context is the place where data will be moved to. This can be either
/// interrupt context, or exception context
pub enum Context<I> {
    /// An Exception, such as SysTick. Re-exported from the `cortex-m` crate
    Exception(Exception),

    /// A device specific interrupt, as defined by a `-pac` crate
    Interrupt(I),
}

impl<I: Nr> PartialEq<VectActive> for Context<I> {
    fn eq(&self, other: &VectActive) -> bool {
        match (self, other) {
            (Context::Exception(e_s), VectActive::Exception(e_o)) => e_s == e_o,
            (Context::Interrupt(i_s), VectActive::Interrupt{ irqn }) => i_s.nr() == *irqn,
            _ => false,
        }
    }
}

/// Move is a structure that is intended to be stored as a static variable,
/// and represents a metaphorical "move" to an interrupt context. Data is moved
/// to the interrupt context by calling `try_move` from thread (non-interrupt)
/// context, and the data can be retrived within a selected interrupt using the
/// `try_lock` method.
pub struct Move<T, I> {
    /// `data` contains the user data, which may or may not be initialized
    data: UnsafeCell<MaybeUninit<T>>,

    // `state` is a runtime tracking of our current state.
    state: AtomicU8,

    context: Context<I>,
}

unsafe impl<T, I> Sync for Move<T, I>
where
    T: Send + Sized,
    I: Nr,
{
}

impl<T, I> Move<T, I> {
    /// The data is uninitialized
    const UNINIT: u8 = 0;

    /// The data is initialized and not currently locked
    const INIT_AND_IDLE: u8 = 1;

    /// The data is initialized, but currently locked by an interrupt
    const LOCKED: u8 = 2;

    /// Create a new `Move` structure without initializing the data contained by it.
    /// This is best used when the data cannot be initialized until runtime, such as
    /// a HAL peripheral, or the producer or consumer of a queue.
    ///
    /// Before using this in interrupt context, you must initialize it with the
    /// `try_move` function, or it will return errors upon access.
    ///
    /// You must provide the context that is allowed to later access this data
    /// as the `ctxt` argument
    pub const fn new_uninitialized(ctxt: Context<I>) -> Self {
        Move {
            data: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(Self::UNINIT),
            context: ctxt,
        }
    }

     /// Create a new `Move` structure, and initialize the data contained within it.
     /// This is best used when the data contained within is `const`, and doesn't require
     /// runtime initialization.
     ///
     /// This does not require further interaction before use in interrupt context.
     ///
     /// You must provide the context that is allowed to later access this data
     /// as the `ctxt` argument
    pub const fn new(data: T, ctxt: Context<I>) -> Self {
        Move {
            data: UnsafeCell::new(MaybeUninit::new(data)),
            state: AtomicU8::new(Self::UNINIT),
            context: ctxt,
        }
    }
}

impl<T, I> Move<T, I>
where
    T: Send + Sized,
    I: Nr,
{
    /// Attempt to initialize the data of the `Move` structure.
    /// This *MUST* be called from non-interrupt context, and a critical
    /// section will be in place while setting the data.
    ///
    /// Returns:
    ///
    /// * Ok(Some(T)): If we are in thread mode and the data was previously initialized
    /// * Ok(None): If we are in thread mode and the data was not previously initialized
    /// * Err(T): If we are not in thread mode (e.g. an interrupt is active), return the
    ///     data that was going to be moved
    pub fn try_move(&self, data: T) -> Result<Option<T>, T> {
        free(|_cs| {
            // Check if we are in non-interrupt context
            match SCB::vect_active() {
                // TODO: Would it be reasonable to initialize this from a DIFFERENT
                // interrupt context? Basically anything but the destination interrupt?
                VectActive::ThreadMode => {}
                _ => {
                    return Err(data);
                }
            }

            // Since we are in a critical section, it is not necessary to perform
            // an atomic compare and swap, as we cannot be pre-empted
            match self.state.load(Ordering::SeqCst) {
                Self::UNINIT => {
                    unsafe {
                        // Reference to an uninitialized MaybeUninit
                        let mu_ref = &mut *self.data.get();

                        // Get a pointer to the data, and use ptr::write to avoid
                        // viewing or creating a reference to uninitialized data
                        let dat_ptr = mu_ref.as_mut_ptr();
                        dat_ptr.write(data);
                    }
                    self.state.store(Self::INIT_AND_IDLE, Ordering::SeqCst);
                    Ok(None)
                }
                Self::INIT_AND_IDLE => {
                    let old = unsafe {
                        // Reference to an initialized MaybeUninit
                        let mu_ref = &mut *self.data.get();

                        // Get a pointer to the data, and use ptr::replace,
                        // a mem::swap is probably okay since this is initialized,
                        // but use ptr methods anyway
                        let dat_ptr = mu_ref.as_mut_ptr();
                        dat_ptr.replace(data)
                    };
                    Ok(Some(old))
                }
                Self::LOCKED | _ => Err(data),
            }
        })
    }

    /// Attempt to recover the data from the `Move` structure.
    /// This *MUST* be called from non-interrupt context, and a critical
    /// section will be in place while receiving the data.
    ///
    /// Returns:
    ///
    /// * Ok(Some(T)): If we are in thread mode and the data was previously initialized
    /// * Ok(None): If we are in thread mode and the data was not previously initialized
    /// * Err(()): If we are not in thread mode (e.g. an interrupt is active)
    pub fn try_free(&self) -> Result<Option<T>, ()> {
        free(|_cs| {
            // Check if we are in non-interrupt context
            match SCB::vect_active() {
                // TODO: Would it be reasonable to free this from a DIFFERENT
                // interrupt context? Basically anything but the destination interrupt?
                VectActive::ThreadMode => {}
                _ => {
                    return Err(());
                }
            }

            // Since we are in a critical section, it is not necessary to perform
            // an atomic compare and swap, as we cannot be pre-empted
            match self.state.load(Ordering::SeqCst) {
                Self::UNINIT => Ok(None),
                Self::INIT_AND_IDLE => {
                    let old = unsafe {
                        // Get a pointer to the initialized data
                        let mu_ptr = self.data.get();

                        // Replace it with an uninitialized field. I winder if this is
                        // just a no-op, or if we should explicitly zero the memory here
                        mu_ptr.replace(MaybeUninit::uninit()).assume_init()
                    };

                    self.state.store(Self::UNINIT, Ordering::SeqCst);

                    Ok(Some(old))
                }
                Self::LOCKED | _ => Err(()),
            }
        })
    }

    /// So, this isn't a classical mutex. It will *only* provide access if:
    ///
    /// * The selected interrupt/exception is currently active
    /// * The mutex has not already been locked
    ///
    /// If these conditions are met, then you can access the variable from within
    /// a closure
    pub fn try_lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, ()> {
        if self.context != SCB::vect_active() {
            return Err(());
        }

        // We know that the current interrupt is active, which means
        // that thread mode cannot resume until we exit this function.
        // We don't need to worry about compare and swap, because we
        // are now the only ones who can access this data
        match self.state.load(Ordering::SeqCst) {
            // The data is uninitialized. Don't provide access
            Self::UNINIT => Err(()),

            // The data is initialized, allow access within a closure
            // This prevents re-entrancy of re-calling lock within the
            // closure
            Self::INIT_AND_IDLE => {
                self.state.store(Self::LOCKED, Ordering::SeqCst);

                let dat_ref = unsafe {
                    // Create a mutable reference to an initialized MaybeUninit
                    let mu_ref = &mut *self.data.get();

                    // Create a mutable reference to the initialized data behind
                    // the MaybeUninit. This is fine, because the scope of this
                    // reference can only live to the end of this function, and
                    // cannot be captured by the closure used below.
                    //
                    // Additionally we have a re-entrancy check above, to prevent
                    // creating a duplicate &mut to the inner data
                    let dat_ptr = mu_ref.as_mut_ptr();
                    &mut *dat_ptr
                };

                // Call the user's closure, providing access to the data
                let ret = f(dat_ref);

                self.state.store(Self::INIT_AND_IDLE, Ordering::SeqCst);

                Ok(ret)
            }

            // The data is locked, or the status register is garbage.
            // Don't provide access
            Self::LOCKED | _ => Err(()),
        }
    }
}
