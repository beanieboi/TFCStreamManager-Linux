use gtk4::prelude::*;
use std::rc::Rc;

use super::{MainWindow, ServerContext};
use crate::competition::MatchFormat;
use crate::services::{OverlayTemplate, TemplateFormat, select_overlay};

impl MainWindow {
    /// The selection key whose template `/` serves: that of the match on
    /// the table (for MTFV with its format), or the chosen competition while
    /// there is none (ADR 0012).
    fn overlay_source(context: &ServerContext) -> Option<String> {
        context.overlay.overlay_key().or_else(|| {
            context
                .runtime
                .block_on(async { context.settings.read().await.competition })
                .map(|c| c.key().to_owned())
        })
    }

    /// Fills the overlay dropdown with all templates and selects the one `/`
    /// serves. Without a competition it only shows the default.
    pub(super) fn show_overlays(context: &ServerContext) {
        let picker = &context.overlay_picker;
        let source = Self::overlay_source(context);
        let templates = picker.library.templates();
        let choices = context
            .runtime
            .block_on(async { context.settings.read().await.overlay_choices.clone() });
        let selected_template = select_overlay(&templates, source.as_deref(), &choices);
        picker.format.set(selected_template.and_then(|t| t.format));
        let selected = selected_template.map(|t| t.id.clone());
        context.overlay.set_selected_template(selected.clone());

        // Only templates that can show the match on the table are offered:
        // its format (an MTFV league) or its players (singles, doubles).
        let constrained = source.as_deref().is_some_and(|key| key.contains(':'));
        picker.updating.set(true);
        picker.combo.remove_all();
        for template in &templates {
            if constrained
                && !source
                    .as_deref()
                    .is_some_and(|key| template.fits_match(key))
            {
                continue;
            }
            picker.combo.append(Some(&template.id), &template.name);
        }
        picker.combo.set_active_id(selected.as_deref());
        picker.combo.set_sensitive(source.is_some());
        picker.updating.set(false);

        Self::show_overlay_url(context);
        Self::adapt_frei_form(context, &templates, &choices);
        Self::apply_template_format(context);
    }

    /// A Kickertool or Frei match takes the format of the template on air:
    /// a race template makes it a race to the configured target, a set
    /// template a set match (MTFV formats come from the league). Called on
    /// every change, so a new match follows too; it only acts on a mismatch.
    pub(super) fn apply_template_format(context: &ServerContext) {
        let Some(wanted) = context.overlay_picker.format.get() else {
            return;
        };
        // Kickertool only: a Frei match's kind is chosen in its panel, an
        // MTFV format comes from the league.
        let Some(current) = context
            .match_services
            .store
            .scoreboard()
            .filter(|s| s.competition == "kickertool")
            .and_then(|s| s.match_format)
        else {
            return;
        };
        let wanted = match wanted {
            TemplateFormat::Sets => MatchFormat::Sets {},
            TemplateFormat::Race => {
                let (target, segment) = context.race_settings();
                MatchFormat::Race { target, segment }
            }
        };
        if current != wanted {
            Self::run_command(context, |m, revision| m.set_format(wanted, revision));
        }
    }

    /// Remembers a template chosen by hand for the current competition.
    pub(super) fn connect_overlay_picker(context: Rc<ServerContext>) {
        let combo = context.overlay_picker.combo.clone();
        combo.connect_changed(move |combo| {
            let picker = &context.overlay_picker;
            if picker.updating.get() {
                return;
            }
            let (Some(source), Some(id)) = (Self::overlay_source(&context), combo.active_id())
            else {
                return;
            };
            context.runtime.block_on(async {
                context
                    .settings
                    .write()
                    .await
                    .overlay_choices
                    .insert(source, id.to_string());
            });
            Self::save_settings(&context);
            context.overlay.set_selected_template(Some(id.to_string()));
            Self::show_overlay_url(&context);
            let templates = picker.library.templates();
            let choices = context
                .runtime
                .block_on(async { context.settings.read().await.overlay_choices.clone() });
            Self::adapt_frei_form(&context, &templates, &choices);
            let format = templates
                .iter()
                .find(|t| t.id == id.as_str())
                .and_then(|t| t.format);
            picker.format.set(format);
            Self::apply_template_format(&context);
        });
    }

    /// The template's own URL, for an OBS source that should not follow the
    /// app.
    fn show_overlay_url(context: &ServerContext) {
        let combo = &context.overlay_picker.combo;
        let tooltip = combo.active_id().map(|id| {
            let port = context
                .runtime
                .block_on(async { context.settings.read().await.port });
            format!("Overlay-Vorlage\n{}/overlay/{id}", Self::overlay_url(port))
        });
        combo.set_tooltip_text(Some(tooltip.as_deref().unwrap_or("Overlay-Vorlage")));
    }

    /// Disables the Frei fields the template for Frei has no placeholder
    /// for. They stay visible, so the layout does not jump.
    fn adapt_frei_form(
        context: &ServerContext,
        templates: &[OverlayTemplate],
        choices: &std::collections::BTreeMap<String, String>,
    ) {
        let kind = context
            .runtime
            .block_on(async { context.settings.read().await.frei_kind });
        let Some(template) = select_overlay(templates, Some(kind.overlay_key()), choices) else {
            return;
        };
        let f = &context.frei;
        let fields = [
            (&f.tournament, "tournamentName"),
            (&f.discipline, "disciplineName"),
            (&f.round, "roundName"),
            (&f.group, "groupName"),
        ];
        let unused = format!("Wird von der Vorlage „{}“ nicht angezeigt", template.name);
        for (entry, placeholder) in fields {
            let used = template.uses(placeholder);
            entry.set_sensitive(used);
            entry.set_tooltip_text((!used).then_some(unused.as_str()));
        }
    }

    pub(super) fn save_settings(context: &ServerContext) {
        let settings = context
            .runtime
            .block_on(async { context.settings.read().await.clone() });
        if let Err(e) = context.settings_service.save(&settings) {
            tracing::error!("Failed to save settings: {}", e);
        }
    }
}
