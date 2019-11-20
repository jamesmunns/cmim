#![no_std]
#![no_main]

// Panic provider crate
use panic_halt as _;

// String formatting
use cmim::Move;
use core::fmt::Write;
use heapless::String as HString;

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

static UART: Move<Uarte<UARTE0>, Interrupt> = Move::new_uninitialized(Interrupt::TIMER1);
static TIMER: Move<Timer<TIMER1>, Interrupt> = Move::new_uninitialized(Interrupt::TIMER1);

#[entry]
fn main() -> ! {
    let mut board = DWM1001::take().unwrap();
    let mut timer = board.TIMER0.constrain();
    let mut _rng = board.RNG.constrain();

    let mut itimer = board.TIMER1.constrain();
    itimer.start(1_000_000u32);
    itimer.enable_interrupt(&mut board.NVIC);

    UART.try_move(board.uart).map_err(drop).unwrap();
    TIMER.try_move(itimer).map_err(drop).unwrap();

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
    // Start the timer again
    TIMER.try_lock(|timer| {
        timer.cancel().unwrap();
        timer.start(1_000_000u32);
    })
    .map_err(drop)
    .unwrap();

    // Print
    UART.try_lock(|uart| {
        let mut s: HString<heapless::consts::U1024> = HString::new();
        write!(&mut s, "Blink!\r\n").unwrap();
        uart.write(s.as_bytes()).unwrap();
    })
    .map_err(drop)
    .unwrap();
}
