use std::process::Command;
use anyhow::{anyhow, Result};
use libpulse_binding::def::BufferAttr;
use libpulse_binding as pulse;
use libpulse_binding::context::introspect::ServerInfo;
use libpulse_binding::stream::Direction;
use libpulse_simple_binding as psimple;
use pulse::sample::{Format, Spec};
use std::slice;
use std::sync::{Arc, Mutex};


fn get_vol() -> f32 {
    let output = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .expect("Failed to execute command");

    String::from_utf8_lossy(&output.stdout)
        .to_string()
        .split_off(8)
        .trim()
        .parse()
        .unwrap()
}

fn set_vol(vol: f32) -> f32 {
    if vol > 1.0 {
        eprintln!("Volume invalid! (Must be between 0 and 1");
        return get_vol();
    }
    let output = Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &vol.to_string()])
        .output()
        .expect("Failed to set the volume.");

    if output.status.success() {
        get_vol()
    } else {
        eprintln!("Command failed with error: {}", output.status);
        eprintln!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
        get_vol()
    }
}

fn get_output_vol() -> Result<f32> {
    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0
        }
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

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

    let byte_buf = unsafe {
        slice::from_raw_parts_mut(
            buf.as_mut_ptr() as *mut u8,
            buf.len() * std::mem::size_of::<i32>(),
        )
    };

    s.read(byte_buf)?;

    float_buf.clear();
    float_buf.extend(buf.iter().map(|s| *s as f32 / i32::MAX as f32));

    Ok(rms(&float_buf))

}

fn main() {
    println!("Getting current system volume...");
    println!("{}", get_vol());
    println!("Setting current volume to 0.3...");
    println!("{}", set_vol(0.3));
    println!("Getting current active output level...");
    println!("{}", get_output_vol().unwrap());
}