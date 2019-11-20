#![no_std]

use bare_metal::Nr;
use core::cell::UnsafeCell;
use core::result::Result;
use cortex_m::interrupt::free;
use cortex_m::peripheral::{
    SCB,
    scb::VectActive,
};
use core::mem::MaybeUninit;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

pub struct Move<T, I>
{
    data: UnsafeCell<MaybeUninit<T>>,
    state: AtomicUsize,
    inter: I
}

unsafe impl<T, I> Sync for Move<T, I>
where
    T: Send + Sized,
    I: Nr
{}

impl<T, I> Move<T, I>
{
    const UNINIT: usize        = 0;
    const INIT_AND_IDLE: usize = 1;
    const LOCKED: usize        = 2;

    pub const fn new_uninitialized(inter: I) -> Self {
        Move {
            data: UnsafeCell::new(MaybeUninit::uninit()),
            inter,
            state: AtomicUsize::new(Self::UNINIT),
        }
    }

    pub const fn new(data: T, inter: I) -> Self {
        Move {
            data: UnsafeCell::new(MaybeUninit::new(data)),
            inter,
            state: AtomicUsize::new(Self::INIT_AND_IDLE),
        }
    }
}

impl<T, I> Move<T, I>
where
    T: Send + Sized,
    I: Nr
{
    /// Attempt to initialize the data of the `Move` structure.
    /// This *MUST* be called from non-interrupt context, and a critical
    /// section will be in place while setting the data.
    ///
    /// Returns:
    ///
    /// * Ok(Some(T)): If we are in thread mode and the data was previously initialized
    /// * Ok(None): If we are in thread mode and the data was not previously initialized
    /// * Err(()): If we are not in thread mode (e.g. an interrupt is active)
    pub fn try_move(&self, data: T) -> Result<Option<T>, T> {
        free(|_cs| {
            // Check if we are in non-interrupt context
            match SCB::vect_active() {
                // TODO: Would it be reasonable to initialize this from a DIFFERENT
                // interrupt context? Basically anything but the destination interrupt?
                VectActive::ThreadMode => {},
                _ => {
                    return Err(data);
                }
            }

            // Since we are in a critical section, it is not necessary to perform
            // an atomic compare and swap, as we cannot be pre-empted
            match self.state.load(Ordering::SeqCst) {
                Self::UNINIT => {
                    unsafe {
                        let mu_ref = &mut *self.data.get();
                        let dat_ptr = mu_ref.as_mut_ptr();
                        dat_ptr.write(data);
                    }
                    self.state.store(Self::INIT_AND_IDLE, Ordering::SeqCst);
                    Ok(None)
                }
                Self::INIT_AND_IDLE => {
                    let old = unsafe {
                        let mu_ref = &mut *self.data.get();
                        let dat_ptr = mu_ref.as_mut_ptr();
                        dat_ptr.replace(data)
                    };
                    Ok(Some(old))
                }
                Self::LOCKED | _ => {
                    Err(data)
                }
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
                VectActive::ThreadMode => {},
                _ => {
                    return Err(());
                }
            }

            // Since we are in a critical section, it is not necessary to perform
            // an atomic compare and swap, as we cannot be pre-empted
            match self.state.load(Ordering::SeqCst) {
                Self::UNINIT => {
                    Ok(None)
                }
                Self::INIT_AND_IDLE => {
                    let old = unsafe {
                        let mu_ptr = self.data.get();
                        mu_ptr.replace(MaybeUninit::uninit()).assume_init()
                    };

                    self.state.store(Self::UNINIT, Ordering::SeqCst);

                    Ok(Some(old))
                }
                Self::LOCKED | _ => {
                    Err(())
                }
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
            VectActive::Interrupt{ irqn } if irqn == self.inter.nr() => {
                // Okay to go ahead
            }
            _ => {
                return Err(())
            }
        };

        // We know that the current interrupt is active, which means
        // that thread mode cannot resume until we exit this function.
        // We don't need to worry about compare and swap, because we
        // are now the only ones who can access this data
        match self.state.load(Ordering::SeqCst) {

            // The data is uninitialized. Don't provide access
            Self::UNINIT => {
                Err(())
            }

            // The data is initialized, allow access within a closure
            // This prevents re-entrancy of re-calling lock within the
            // closure
            Self::INIT_AND_IDLE => {
                self.state.store(Self::LOCKED, Ordering::SeqCst);

                let dat_ref = unsafe {
                    let mu_ref = &mut *self.data.get();
                    let dat_ptr = mu_ref.as_mut_ptr();
                    &mut *dat_ptr
                };

                let ret = f(dat_ref);

                self.state.store(Self::INIT_AND_IDLE, Ordering::SeqCst);

                Ok(ret)
            }

            // The data is locked, or the status register is garbage.
            // Don't provide access
            Self::LOCKED | _ => {
                Err(())
            }
        }
    }
}
