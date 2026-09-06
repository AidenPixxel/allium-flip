use futures::stream::StreamExt;
use std::fmt::Display;
use sysfs_gpio::{Direction, Edge, Error, Pin};

enum AudioOutput {
    Usb = 0,
    InternalSpeaker = 1,
}

impl From<u8> for AudioOutput {
    fn from(value: u8) -> Self {
        match value {
            0 => AudioOutput::Usb,
            _ => AudioOutput::InternalSpeaker,
        }
    }
}

impl Display for AudioOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioOutput::Usb => write!(f, "USB Audio"),
            AudioOutput::InternalSpeaker => write!(f, "Internal Speaker"),
        }
    }
}

pub async fn usb_audio_task() -> Result<(), Error> {
    let gpio45 = Pin::new(45);
    gpio45.export()?;
    gpio45.set_direction(Direction::In)?;
    // Both is not supported on this device. Setting to Rising/Falling will return both edges.
    gpio45.set_edge(Edge::RisingEdge)?;

    if let Ok(value) = gpio45.get_value() {
        set_audio_output(value.into())?;
    }

    let mut gpio_events = gpio45.get_value_stream()?;

    while let Some(evt) = gpio_events.next().await {
        if let Ok(value) = evt {
            set_audio_output(value.into())?;
        }
    }
    Ok(())
}

fn set_audio_output(output: AudioOutput) -> Result<(), Error> {
    log::info!("Audio output is: {}", output);

    let gpio44 = Pin::new(44);
    gpio44.export()?;
    gpio44.set_value(output as u8)
}
