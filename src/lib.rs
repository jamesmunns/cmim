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
//! # #![no_main]
//! # #[derive(Debug)]
//! # struct Foo;
//! # #[derive(Debug)]
//! # struct Bar;
//! # impl Bar {
//! #     fn with_settings(_a: u32, _b: u32) -> Self { unimplemented!() }
//! #     fn some_bar_method(&mut self) { unimplemented!() }
//! # }
//! # impl Foo {
//! #     fn some_foo_method(&mut self) { unimplemented!() }
//! #     fn default() -> Self { unimplemented!() }
//! # }
//! use nrf52832_hal::nrf52832_pac::{self, interrupt, Interrupt, NVIC};
//! use cortex_m::asm::wfi;
//! use cortex_m_rt::entry;
//! use cmim::{Move};
//!
//! // Define your global variables, and what
//! // interrupt they are allowed to be used from
//!
//! // These variables are initialized at runtime
//! static FOO: Move<Foo, Interrupt> = Move::new_uninitialized(Interrupt::UARTE0_UART0);
//! static BAR: Move<Bar, Interrupt> = Move::new_uninitialized(Interrupt::UARTE0_UART0);
//!
//! // These variables are const initialized. This probably isn't super useful vs just
//! // having a static variable inside the function, but would allow you to later send
//! // new data to the interrupt.
//! static BAZ: Move<u64, Interrupt> = Move::new(123u64, Interrupt::TIMER0);
//!
//! #[entry]
//! fn main() -> ! {
//!     let periphs = nrf52832_pac::CorePeripherals::take().unwrap();
//!
//!     let mut nvic = periphs.NVIC;
//!
//!     // Data *MUST* be initialized from non-interrupt context. A critical
//!     // section will be used when initializing the data.
//!     //
//!     // Since this data has never been initialized before, these will return Ok(None).
//!     assert!(FOO.try_move(Foo::default()).unwrap().is_none());
//!     assert!(BAR.try_move(Bar::with_settings(123, 456)).unwrap().is_none());
//!
//!     // Since this data WAS initialized, we will get the old data back as Ok(Some(T))
//!     assert_eq!(BAZ.try_move(456u64), Ok(Some(123u64)));
//!
//!     // Now you can enable the interrupts
//!     unsafe {
//!         NVIC::unmask(Interrupt::UARTE0_UART0);
//!         NVIC::unmask(Interrupt::TIMER0);
//!     }
//!
//!     loop {
//!         wfi();
//!     }
//! }
//!
//! #[interrupt]
//! fn UARTE0_UART0() {
//!     // You're allowed to access any data you've
//!     // "given" to the interrupt. You'll get a
//!     // mutable reference to your data inside of
//!     // a closure.
//!     //
//!     // You can either stack closures like this, or
//!     // just use a single struct containing all data.
//!     //
//!     // You will only get an error if:
//!     //
//!     // 1. You try to lock the same data multiple times
//!     // 2. You try to lock the data from the wrong interrupt
//!     // 3. You never initialized the data
//!     //
//!     // If you avoid these three things, it should always be
//!     // safe to unwrap the return values
//!     FOO.try_lock(|foo| {
//!         BAR.try_lock(|bar| {
//!             uart0_inner(foo, bar);
//!         }).unwrap();
//!     }).unwrap();
//! }
//!
//! fn uart0_inner(foo: &mut Foo, bar: &mut Bar) {
//!     foo.some_foo_method();
//!     bar.some_bar_method();
//! }
//!
//! #[interrupt]
//! fn TIMER0() {
//!     BAZ.try_lock(|baz| {
//!         // Do something with baz...
//!     }).unwrap();
//!
//!     // This doesn't work, and will panic at
//!     // runtime because it is the wrong interrupt
//!     //
//!     // FOO.try_lock(|foo| {
//!     //     // Do something with foo...
//!     // }).unwrap();
//! }
//!
//! fn not_an_interrupt() {
//!     // This doesn't work, and will panic at
//!     // runtime because we're not in an interrupt
//!     //
//!     // FOO.try_lock(|foo| {
//!     //     // Do something with foo...
//!     // }).unwrap();
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
    mem::MaybeUninit,
    result::Result,
    sync::atomic::{AtomicU8, Ordering},
};

use bare_metal::Nr;
use cortex_m::interrupt::free;
use cortex_m::peripheral::{scb::VectActive, SCB};

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

    // `inter` is the interrupt type. This type is unique to every chip
    // as it is generated by svd2rust, but all types implement the `Nr`
    // trait
    inter: I,
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
    /// You must provide the interrupt that is allowed to later access this data
    /// as the `inter` argument
    pub const fn new_uninitialized(inter: I) -> Self {
        Move {
            data: UnsafeCell::new(MaybeUninit::uninit()),
            inter,
            state: AtomicU8::new(Self::UNINIT),
        }
    }

    /// Create a new `Move` structure, and initialize the data contained within it.
    /// This is best used when the data contained within is `const`, and doesn't require
    /// runtime initialization.
    ///
    /// This does not require further interaction before use in interrupt context.
    ///
    /// You must provide the interrupt that is allowed to later access this data
    /// as the `inter` argument
    pub const fn new(data: T, inter: I) -> Self {
        Move {
            data: UnsafeCell::new(MaybeUninit::new(data)),
            inter,
            state: AtomicU8::new(Self::INIT_AND_IDLE),
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
    /// * The selected interrupt is currently active
    /// * The mutex has not already been locked
    ///
    /// If these conditions are met, then you can access the variable from within
    /// a closure
    pub fn try_lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, ()> {
        match SCB::vect_active() {
            VectActive::Interrupt { irqn } if irqn == self.inter.nr() => {
                // Okay to go ahead
            }
            _ => return Err(()),
        };

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
