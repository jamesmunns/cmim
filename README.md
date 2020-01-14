# Cortex-M Interrupt Move

It's the next best thing to moving to interrupt context.

## Examples

Check out the full examples in the [`app-examples`](./app-examples) folder.

## The goal

The goal here is to replace usage of a mutex which may require an entire critical section, and instead model "Moving" of data to an interrupt context.

This means that we don't need a critical section to access it, we just need to be in the interrupt we moved the data to.

Here's how it works:

```rust
use nrf52832_hal::nrf52832_pac::{self, interrupt, Interrupt, NVIC};
use cortex_m::asm::wfi;
use cortex_m_rt::entry;
use cmim::{Move};

// Define your global variables, and what
// interrupt they are allowed to be used from

// These variables are initialized at runtime
static FOO: Move<Foo, Interrupt> = Move::new_uninitialized(Interrupt::UARTE0_UART0);
static BAR: Move<Bar, Interrupt> = Move::new_uninitialized(Interrupt::UARTE0_UART0);

// These variables are const initialized. This probably isn't super useful vs just
// having a static variable inside the function, but would allow you to later send
// new data to the interrupt.
static BAZ: Move<u64, Interrupt> = Move::new(123u64, Interrupt::TIMER0);

#[entry]
fn main() -> ! {
    let periphs = nrf52832_pac::CorePeripherals::take().unwrap();

    let mut nvic = periphs.NVIC;

    // Data *MUST* be initialized from non-interrupt context. A critical
    // section will be used when initializing the data.
    //
    // Since this data has never been initialized before, these will return Ok(None).
    assert!(FOO.try_move(Foo::default()).unwrap().is_none());
    assert!(BAR.try_move(Bar::with_settings(123, 456)).unwrap().is_none());

    // Since this data WAS initialized, we will get the old data back as Ok(Some(T))
    assert_eq!(BAZ.try_move(456u64), Ok(Some(123u64)));

    // Now you can enable the interrupts
    unsafe {
        NVIC::unmask(Interrupt::UARTE0_UART0);
        NVIC::unmask(Interrupt::TIMER0);
    }

    loop {
        wfi();
    }
}

#[interrupt]
fn UARTE0_UART0() {
    // You're allowed to access any data you've
    // "given" to the interrupt. You'll get a
    // mutable reference to your data inside of
    // a closure.
    //
    // You can either stack closures like this, or
    // just use a single struct containing all data.
    //
    // You will only get an error if:
    //
    // 1. You try to lock the same data multiple times
    // 2. You try to lock the data from the wrong interrupt
    // 3. You never initialized the data
    //
    // If you avoid these three things, it should always be
    // safe to unwrap the return values
    FOO.try_lock(|foo| {
        BAR.try_lock(|bar| {
            uart0_inner(foo, bar);
        }).unwrap();
    }).unwrap();
}

fn uart0_inner(foo: &mut Foo, bar: &mut Bar) {
    foo.some_foo_method();
    bar.some_bar_method();
}

#[interrupt]
fn TIMER0() {
    BAZ.try_lock(|baz| {
        // Do something with baz...
    }).unwrap();

    // This doesn't work, and will panic at
    // runtime because it is the wrong interrupt
    //
    // FOO.try_lock(|foo| {
    //     // Do something with foo...
    // }).unwrap();
}

fn not_an_interrupt() {
    // This doesn't work, and will panic at
    // runtime because we're not in an interrupt
    //
    // FOO.try_lock(|foo| {
    //     // Do something with foo...
    // }).unwrap();
}
```


# License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
