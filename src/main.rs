use anyhow::{anyhow, Result};
use libpulse_binding::def::BufferAttr;
use libpulse_binding as pulse;
use libpulse_binding::context::introspect::ServerInfo;
use libpulse_binding::stream::Direction;
use libpulse_simple_binding as psimple;
use pulse::sample::{Format, Spec};
use std::io::Write;
use std::{io, slice};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

fn main() -> Result<()> {
    // pa setup
    let spec = Spec {
        format: Format::S32le,
        rate: 48000,
        channels: 2,
    };
    assert!(spec.is_valid());

    let mut mainloop = pulse::mainloop::standard::Mainloop::new().unwrap();
    let mut context = pulse::context::Context::new(&mainloop, "auto_volume").unwrap();
    context.connect(None, pulse::context::FlagSet::NOFLAGS, None)?;

    // wait 4 context
    loop {
        mainloop.iterate(true); // block
        match context.get_state() {
            pulse::context::State::Ready => break,
            pulse::context::State::Failed | pulse::context::State::Terminated => {
                return Err(anyhow!("Could not connect to PulseAudio"));
            }
            _ => {}
        }
    }

    // the worst. holy fuck. fuck. shit. no.
    let sink_name = Arc::new(Mutex::new(None));
    let done = Arc::new(Mutex::new(false));

    {
        let sink_name = Arc::clone(&sink_name);
        let done = Arc::clone(&done);

        context.introspect().get_server_info(move |info: &ServerInfo| {
            if let Some(name) = &info.default_sink_name {
                *sink_name.lock().unwrap() = Some(name.to_string());
            }
            *done.lock().unwrap() = true;
        });
    }

    // wait for callback
    while !*done.lock().unwrap() {
        mainloop.iterate(true);
    }

    let sink_name = sink_name.lock().unwrap().clone().ok_or(anyhow!("No default sink found"))?;
    let monitor_source = format!("{}.monitor", sink_name);
    println!("Using monitor source: {}", monitor_source);

    let attr = BufferAttr {
        maxlength: 1024 * 4,    // max buffer size in bytes
        tlength: 1024 * 2,      // target length
        prebuf: 0,              // how much to prefill before playback/record
        minreq: 1024,           // minimum request size
        fragsize: 1024,         // fragment size
    };

    let s = psimple::Simple::new(
        None,
        "auto_volume",
        Direction::Record,
        Some(monitor_source.as_str()),
        "Monitor Capture",
        &spec,
        None,
        Some(&attr),
    )?;

    // buffer
    let mut buf = [0i32; 512]; // match S32le
    let mut float_buf = Vec::with_capacity(buf.len());

    //main
    loop {
        let byte_buf = unsafe {
            slice::from_raw_parts_mut(
                buf.as_mut_ptr() as *mut u8,
                buf.len() * std::mem::size_of::<i32>(),
            )
        };

        s.read(byte_buf)?;

        // Convert to f32 normalized samples
        float_buf.clear();
        float_buf.extend(buf.iter().map(|s| *s as f32 / i32::MAX as f32));

        let level = rms(&float_buf);

        // bar
        let bar_len = (level * 50.0).round() as usize;
        let bar = "█".repeat(bar_len);

        // Print bar, overwrite the same line
        print!("\r[{:<50}] {:.0}%", bar, level * 100.0);
        io::stdout().flush().unwrap(); // Force terminal to display immediately

        // update fast. otherwise it gets fucked up
        thread::sleep(Duration::from_millis(1));
    }
}
