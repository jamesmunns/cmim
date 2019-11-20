#![no_std]
#![no_main]

// Panic provider crate
use panic_halt as _;

// String formatting
use cmim::Move;

// Used to set the program entry point
use cortex_m_rt::entry;

use embedded_hal::timer::Cancel;

// Provides definitions for our development board
use dwm1001::{
    nrf52832_hal::{
        nrf52832_pac::{interrupt, Interrupt, UARTE0, TIMER1},
        prelude::*,
        Uarte,
        Timer,
    },
    DWM1001,
};

static TIMER_1_DATA: Move<Timer1Data, Interrupt> = Move::new_uninitialized(Interrupt::TIMER1);

struct Timer1Data {
    uart: Uarte<UARTE0>,
    timer: Timer<TIMER1>,
}

#[entry]
fn main() -> ! {
    let mut board = DWM1001::take().unwrap();
    let mut timer = board.TIMER0.constrain();
    let mut _rng = board.RNG.constrain();

    let mut itimer = board.TIMER1.constrain();
    itimer.start(1_000_000u32);
    itimer.enable_interrupt(&mut board.NVIC);

    TIMER_1_DATA.try_move(Timer1Data {
        uart: board.uart,
        timer: itimer,
    }).map_err(drop).unwrap();

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

#[interrupt]
fn TIMER1() {
    TIMER_1_DATA.try_lock(|data| {
        // Start the timer again first for accuracy
        data.timer.cancel().unwrap();
        data.timer.start(1_000_000u32);

        // Write message to UART. The NRF UART requires data
        // to be in RAM, not flash.
        const MSG_BYTES: &[u8] = "Blink!\r\n".as_bytes();
        let mut buf = [0u8; MSG_BYTES.len()];
        buf.copy_from_slice(MSG_BYTES);

        data.uart.write(&buf).unwrap();
    })
    .map_err(drop)
    .unwrap();
}
