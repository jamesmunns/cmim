# Cortex-M Interrupt Move

It's the next best thing to moving to interrupt context.

## Examples

Check out the full examples in the [`app-examples`](./app-examples) folder.

## The goal

The goal here is to replace usage of a mutex which may require an entire critical section, and instead model "Moving" of data to an interrupt context.

This means that we don't need a critical section to access it, we just need to be in the interrupt we moved the data to.

Here's how it works:

```rust
#![no_std]
#![no_main]

// Panic provider crate
use panic_halt as _;

// CMIM items
use cmim::{
    Move,
    Context,
    Exception,
};

// Used to set the program entry point
use cortex_m_rt::{entry, exception};

use embedded_hal::timer::Cancel;

// Provides definitions for our development board
use dwm1001::{
    cortex_m::peripheral::syst::SystClkSource::Core,
    nrf52832_hal::{
        nrf52832_pac::{interrupt, Interrupt, TIMER1, UARTE0},
        prelude::*,
        Timer, Uarte,
    },
    DWM1001,
};

static TIMER_1_DATA: Move<Timer1Data, Interrupt> = Move::new_uninitialized(Context::Interrupt(Interrupt::TIMER1));
static SYSTICK_DATA: Move<SysTickData, Interrupt> = Move::new_uninitialized(Context::Exception(Exception::SysTick));

struct Timer1Data {
    uart: Uarte<UARTE0>,
    timer: Timer<TIMER1>,
    led: dwm1001::Led,
    toggle: bool,
}

struct SysTickData {
    led: dwm1001::Led,
    toggle: bool,
}

#[entry]
fn main() -> ! {
    if let Some(mut board) = DWM1001::take() {
        let mut timer = board.TIMER0.constrain();
        let mut _rng = board.RNG.constrain();

        let mut itimer = board.TIMER1.constrain();
        itimer.start(1_000_000u32);
        itimer.enable_interrupt(&mut board.NVIC);

        // Core clock is 64MHz, blink at 16Hz
        board.SYST.set_clock_source(Core);
        board.SYST.set_reload(4_000_000 - 1);
        board.SYST.enable_counter();
        board.SYST.enable_interrupt();

        TIMER_1_DATA
            .try_move(Timer1Data {
                uart: board.uart,
                timer: itimer,
                led: board.leds.D9,
                toggle: false,
            })
            .ok();

        SYSTICK_DATA
            .try_move(SysTickData {
                led: board.leds.D12,
                toggle: false,
            })
            .ok();

        let mut toggle = false;

        loop {
            // board.leds.D9  - Top LED GREEN
            // board.leds.D12 - Top LED RED
            // board.leds.D11 - Bottom LED RED
            // board.leds.D10 - Bottom LED BLUE
            if toggle {
                board.leds.D10.enable();
            } else {
                board.leds.D10.disable();
            }

            toggle = !toggle;

            timer.delay(250_000);
        }
    }

    loop {
        continue;
    }
}

#[exception]
fn SysTick() {
    SYSTICK_DATA
        .try_lock(|data| {
            // Blink the LED
            if data.toggle {
                data.led.enable();
            } else {
                data.led.disable();
            }

            data.toggle = !data.toggle;
        })
        .ok();
}

#[interrupt]
fn TIMER1() {
    TIMER_1_DATA
        .try_lock(|data| {
            // Start the timer again first for accuracy
            data.timer.cancel().unwrap();
            data.timer.start(1_000_000u32);

            // Write message to UART. The NRF UART requires data
            // to be in RAM, not flash.
            const MSG_BYTES: &[u8] = "Blink!\r\n".as_bytes();
            let mut buf = [0u8; MSG_BYTES.len()];
            buf.copy_from_slice(MSG_BYTES);

            data.uart.write(&buf).unwrap();

            // Blink the LED
            if data.toggle {
                data.led.enable();
            } else {
                data.led.disable();
            }

            data.toggle = !data.toggle;
        })
        .ok();
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
