//! The desktop's sound outputs, asked of `pactl`.

use std::process::{Command, Stdio};

/// A sound output, as PipeWire (through pipewire-pulse) or PulseAudio
/// names it.
#[derive(Clone, PartialEq)]
pub struct Output {
    /// The sink name vinospeak's `--audio-device` takes.
    pub name: String,
    pub description: String,
}

/// The desktop's sound outputs, and the default one's name. Empty when
/// `pactl` isn't there, which leaves only the default.
pub fn outputs() -> (Vec<Output>, Option<String>) {
    let run = |args: &[&str]| {
        Command::new("pactl")
            .args(args)
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let sinks = run(&["-f", "json", "list", "sinks"])
        .map(|json| parse_sinks(&json))
        .unwrap_or_default();
    let default = run(&["get-default-sink"]).map(|name| name.trim().to_owned());
    (sinks, default)
}

fn parse_sinks(json: &str) -> Vec<Output> {
    let Ok(serde_json::Value::Array(sinks)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    sinks
        .iter()
        .filter_map(|sink| {
            let name = sink.get("name")?.as_str()?.to_owned();
            let description = sink
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or(&name)
                .to_owned();
            Some(Output { name, description })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sinks_come_from_pactl_json() {
        let json =
            r#"[{"index":1,"name":"hdmi_sink","description":"HDMI 1"},{"name":"bare"},{"x":1}]"#;
        let names: Vec<_> = parse_sinks(json)
            .into_iter()
            .map(|o| (o.name, o.description))
            .collect();
        assert_eq!(
            names,
            [
                ("hdmi_sink".to_owned(), "HDMI 1".to_owned()),
                ("bare".to_owned(), "bare".to_owned())
            ]
        );
        assert!(parse_sinks("not json").is_empty());
    }
}
