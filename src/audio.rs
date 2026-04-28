use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{SharedRb, Producer};

pub struct AudioStream {
    _stream: cpal::Stream,
}

pub fn start_audio() -> Result<(Producer<f32, std::sync::Arc<SharedRb<f32, Vec<std::mem::MaybeUninit<f32>>>>>, AudioStream, u32), Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("No default output device")?;
    let config = device.default_output_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    // Larger ring buffer = more headroom against jitter (less crackle), at the cost of
    // a tiny bit more audio latency. ~16k samples ≈ 340 ms at 48 kHz, which is plenty.
    let rb = SharedRb::<f32, Vec<std::mem::MaybeUninit<f32>>>::new(16384);
    let (mut prod, mut cons) = rb.split();

    // Pre-fill with silence so the very first audio callback doesn't underrun
    // (which produces an audible pop at startup).
    for _ in 0..4096 {
        let _ = prod.push(0.0);
    }

    let _stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _| {
            // cpal hands us an interleaved buffer of `channels` samples per frame.
            // The APU produces mono samples — replicate each mono sample across
            // every channel of one frame, otherwise we'd consume samples at
            // `channels`× the intended rate and alternate them between L/R,
            // producing severe aliasing / static.
            for frame in data.chunks_mut(channels) {
                let s = cons.pop().unwrap_or(0.0);
                for out in frame.iter_mut() {
                    *out = s;
                }
            }
        },
        |err| eprintln!("Audio stream error: {}", err),
        None,
    )?;

    _stream.play()?;

    Ok((prod, AudioStream { _stream }, sample_rate))
}
