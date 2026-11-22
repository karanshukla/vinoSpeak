//! The Settings page, at the sidebar's foot above About: what is set once
//! rather than per reading. The voice blended in and how much, where speech
//! plays, how Save as… saves, Pronunciations, Translation, the Speech API and
//! the voice model. The player keeps Translate, Voice, Speed and Save as….

use eframe::egui;
use vino_ui::a11y;
use vino_ui::theme;
use vino_ui::widgets::{self, Button, ChipKind, Row, Tone};

use super::{App, Page, save};
use crate::outputs::Output;

pub(super) const TITLE: &str = "Settings";
/// A picker's width in a row: room for a voice's name and gender, or a sink's description.
const PICKER_WIDTH: f32 = 240.0;
const SLIDER_WIDTH: f32 = 180.0;

/// What the output picker calls a sink: its description, or its name when
/// `pactl` didn't list it.
fn described(outputs: &[Output], name: &str) -> String {
    outputs
        .iter()
        .find(|o| o.name == name)
        .map_or_else(|| name.to_owned(), |o| o.description.clone())
}

fn default_output_label(outputs: &[Output], default: Option<&str>) -> String {
    match default {
        Some(name) => format!("Default ({})", described(outputs, name)),
        None => "Default output".to_owned(),
    }
}

impl App {
    /// `bar`: the title is up in the window's bar, not here.
    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui, bar: bool) {
        widgets::page_scroll(ui, TITLE, widgets::CONTENT_MAX, |ui| {
            if !bar {
                let title = widgets::title(ui, TITLE);
                a11y::heading(&title, TITLE, 2);
            }
            self.voice_rows(ui);
            self.playback_rows(ui);
            self.saving_rows(ui);
            self.lexicon_rows(ui);
            self.translation_row(ui);
            self.api_rows(ui);
            self.model_row(ui);
        });
    }

    pub(super) fn open_settings(&mut self) {
        self.page = Page::Settings;
        self.model.confirm = false;
        self.refresh_model();
        self.load_lexicon();
        if self.translation.step.is_none() {
            self.refresh_translation();
        }
    }

    /// The sidebar's Settings and About: a second click goes back to the file.
    pub(super) fn toggle_page(&mut self, page: Page) {
        if self.page == page {
            self.page = Page::Reader;
        } else if page == Page::Settings {
            self.open_settings();
        } else {
            self.open_about();
        }
    }

    /// Blend with and Mix: a second voice mixed into the player's Voice.
    fn voice_rows(&mut self, ui: &mut egui::Ui) {
        widgets::group_label(ui, "Voice");
        widgets::group(ui, |ui| {
            Row::new("Blend with")
                .help("Blend a second speaker into the one selected in the player.")
                .show(ui, |ui, _| self.blend_picker(ui));
            let help = self.mix_said();
            Row::new("Mix")
                .help(&help)
                .show(ui, |ui, _| self.mix_slider(ui));
        });
    }

    /// The player's voice and the blended one, in tenths: `Heart 70% · George 30%`.
    fn mix_said(&self) -> String {
        match &self.blend {
            Some(blend) => {
                let first = 10 - u32::from(self.mix);
                format!(
                    "{} {}% · {} {}%",
                    self.voice.name,
                    first * 10,
                    blend.name,
                    u32::from(self.mix) * 10
                )
            }
            None => "Select a second speaker above to adjust the ratio.".to_owned(),
        }
    }

    /// A second voice, offered as the voice picker offers voices.
    fn blend_picker(&mut self, ui: &mut egui::Ui) {
        let language = self.translate_to.map(|t| t.language);
        let label = self.blend.as_ref().map_or_else(
            || "None".to_owned(),
            |v| format!("{} · {}", v.name, v.gender),
        );
        let shown = widgets::combo(ui, "blend", label, PICKER_WIDTH).show_ui(ui, |ui| {
            ui.set_min_width(220.0);
            ui.selectable_value(&mut self.blend, None, "None");
            let speaks = |v: &&crate::voices::Voice| language.is_none_or(|l| v.language == l);
            for voice in self.voices.iter().filter(speaks) {
                if voice.id != self.voice.id {
                    let label = format!("{}  ·  {}", voice.name, voice.gender);
                    ui.selectable_value(&mut self.blend, Some(voice.clone()), label);
                }
            }
        });
        ui.ctx()
            .accesskit_node_builder(shown.response.id, |node| node.set_label("Blend with"));
        if self.blend.as_ref().is_some_and(|b| b.id == self.voice.id) {
            self.blend = None;
        }
    }

    /// How much of the blended voice is heard, in tenths; let go to apply.
    fn mix_slider(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().slider_width = SLIDER_WIDTH;
        let mut mix = self.mix;
        let slider = egui::Slider::new(&mut mix, 1..=crate::voices::MIX_TENTHS)
            .show_value(false)
            .step_by(1.0);
        let shown = ui.add_enabled(self.blend.is_some(), slider);
        ui.ctx()
            .accesskit_node_builder(shown.id, |node| node.set_label("Mix"));
        self.mix = mix;
    }

    /// Where speech plays.
    fn playback_rows(&mut self, ui: &mut egui::Ui) {
        widgets::group_label(ui, "Playback");
        widgets::group(ui, |ui| {
            Row::new("Output")
                .help("Audio playback device. Changes apply starting at the current sentence.")
                .show(ui, |ui, _| self.output_picker(ui));
        });
    }

    fn output_picker(&mut self, ui: &mut egui::Ui) {
        let default = default_output_label(&self.outputs, self.default_output.as_deref());
        let selected = match &self.output {
            Some(name) => described(&self.outputs, name),
            None => default.clone(),
        };
        let shown = widgets::combo(ui, "output", selected, PICKER_WIDTH).show_ui(ui, |ui| {
            ui.selectable_value(&mut self.output, None, default);
            for output in &self.outputs {
                ui.selectable_value(
                    &mut self.output,
                    Some(output.name.clone()),
                    &output.description,
                );
            }
        });
        ui.ctx()
            .accesskit_node_builder(shown.response.id, |node| node.set_label("Output"));
    }

    /// The format and subtitles the player's Save as… saves with.
    fn saving_rows(&mut self, ui: &mut egui::Ui) {
        widgets::group_label(ui, "Save as audio");
        widgets::group(ui, |ui| {
            Row::new("Format")
                .help("Audio file type used when exporting from the player.")
                .show(ui, |ui, _| {
                    let label = save::kind(self.save_format).to_owned();
                    let shown = widgets::combo(ui, "save-format", label, 160.0).show_ui(ui, |ui| {
                        for (suffix, name) in save::FORMATS {
                            ui.selectable_value(&mut self.save_format, suffix, name);
                        }
                    });
                    ui.ctx()
                        .accesskit_node_builder(shown.response.id, |node| node.set_label("Format"));
                });
            Row::new("Subtitles")
                .help("Write a timed .srt caption file alongside the exported audio.")
                .show(ui, |ui, _| {
                    widgets::switch(ui, &mut self.save_subtitles, "Subtitles");
                });
        });
    }
    /// The downloaded model, its size, and Delete… with its confirmation in place.
    fn model_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let bytes = self.model.bytes.unwrap_or(0);
        let reading = self.reading.is_some() || self.saving.is_some();
        if reading {
            self.model.confirm = false;
        }
        let help = self.model.help(bytes, reading);
        let mut row = Row::new("Kokoro voice model").help(&help);
        if self.model.confirm {
            row = row.tint(p.error_tint).tone(Tone::Error);
        } else if self.model.error.is_some() {
            row = row.tone(Tone::Error);
        }
        let mut delete = false;
        let spoken_by = self.vinospeak.display().to_string();
        widgets::group_label(ui, "Model");
        widgets::group(ui, |ui| {
            Row::new("CLI binary").help(&spoken_by).show(ui, |_, _| {});
            row.show(ui, |ui, _| {
                if self.model.deleting {
                    a11y::live(&widgets::chip(ui, ChipKind::Neutral, "Deleting…"));
                } else if self.model.confirm {
                    if ui
                        .add(Button::destructive("Delete the model").small())
                        .clicked()
                    {
                        delete = true;
                    }
                    if ui.add(Button::ghost("Cancel").small()).clicked() {
                        self.model.confirm = false;
                    }
                } else if ui
                    .add_enabled(bytes > 0 && !reading, Button::secondary("Delete…"))
                    .clicked()
                {
                    self.model.confirm = true;
                    self.model.error = None;
                }
            });
        });
        if let Some(said) = &self.model.deleted {
            ui.add_space(8.0);
            a11y::live(&widgets::small(ui, said));
        }
        if delete {
            self.delete_model(ui.ctx());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdmi() -> Vec<Output> {
        vec![Output {
            name: "hdmi_sink".to_owned(),
            description: "HDMI 1".to_owned(),
        }]
    }

    #[test]
    fn an_output_is_called_by_its_description_or_else_its_name() {
        assert_eq!(described(&hdmi(), "hdmi_sink"), "HDMI 1");
        assert_eq!(described(&hdmi(), "usb_dac"), "usb_dac");
    }

    #[test]
    fn the_default_output_is_named_when_pactl_says_which_it_is() {
        assert_eq!(
            default_output_label(&hdmi(), Some("hdmi_sink")),
            "Default (HDMI 1)"
        );
        assert_eq!(default_output_label(&hdmi(), None), "Default output");
    }
}
