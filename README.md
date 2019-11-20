# Cortex-M Interrupt Move

It's the next best thing to moving to interrupt context.

Here's how it should look

```rust
use your_pac::Interrupt;
use crate::{Foo, Bar};

use cmim::{cmim, cmim_get, cmim_set};

// Define your global variables, and what
// interrupt they are allowed to be used from
cmim!{
    FOO: Foo => Interrupt::UART0,
    BAR: Bar => Interrupt::UART0,
    BAZ: u64 => Interrupt::ADC0,
}


#[entry]
fn main() -> ! {
    let periphs = your_pac::CorePeripherals::take().unwrap();

    let mut nvic = periphs.NVIC;
    nvic.disable(Interrupt::UART0);
    nvic.disable(Interrupt::ADC0);

    // You gotta set the variables first.
    // The interrupts can't be enabled yet
    cmim_set!(FOO = Foo::default());
    cmim_set!(BAR = Bar::with_fancy(true));
    cmim_set!(BAZ = 123u64);

    // Now you can enable the interrupts
    nvic.enable(Interrupt::UART0);
    nvic.enable(Interrupt::ADC0);

    loop {
        wfi();
    }
}

#[interrupt]
fn UART0() {
    // You're allowed to access any data you've
    // "given" to the interrupt. You'll get a
    // mutable reference to your data
    //
    // For now it will only panic if you're in
    // the wrong interrupt. In the future, it will
    // also panic if you call it more than once,
    // or if you call `get` without calling `set`
    let foo = cmim_get!(FOO).unwrap();
    let bar = cmim_get!(BAR).unwrap();

    foo.some_foo_method();
    bar.some_mut_bar_method();
}

#[interrupt]
fn ADC0() {
    let baz = cmim_get!(BAZ).unwrap();

    // This doesn't work, and will panic at
    // runtime because it is the wrong interrupt
    //
    // let foo = cmim_get!(FOO).unwrap();
}

fn not_an_interrupt() {
    // This doesn't work, and will panic at
    // runtime because we're not in an interrupt
    //
    // let foo = cmim_get!(FOO).unwrap();
}
```


