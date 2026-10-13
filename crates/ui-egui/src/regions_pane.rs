//! The Editable Regions pane: every content control and permission range the document has, each
//! one a jump (click), an inline content edit and a restore to its first-seen text. Ported from the
//! same-named task pane in VibeOffice's quire; the data comes from `review.regions`.

use std::collections::HashMap;

use egui::{ScrollArea, Ui};
use serde_json::{Value, json};

use crate::WordApp;

/// The pane's state between frames.
#[derive(Default)]
pub struct RegionsPane {
    pub open: bool,
    /// The filter: a region matches by its name, type or text.
    query: String,
    unfilled: bool,
    named_only: bool,
    /// The region being edited inline, the one being renamed, and their shared buffer.
    editing: Option<String>,
    renaming: Option<String>,
    buffer: String,
    message: String,
    /// The first text seen for each region: the Original column and the restore target.
    snapshot: HashMap<String, String>,
}

pub fn pane(app: &mut WordApp, ui: &mut Ui) {
    let mut st = std::mem::take(&mut app.regions);
    let regions = match app.session.run("review.regions", &json!({})) {
        Ok(v) => v["regions"].as_array().cloned().unwrap_or_default(),
        Err(e) => {
            st.message = e.to_string();
            app.regions = st;
            return;
        }
    };
    for r in &regions {
        if let (Some(k), Some(text)) = (str(r, "key"), str(r, "text")) {
            st.snapshot.entry(k.to_string()).or_insert_with(|| text.to_string());
        }
    }

    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut st.query).hint_text(tl!("Filter")).desired_width(140.0));
        ui.checkbox(&mut st.unfilled, tl!("Unfilled only"));
        ui.checkbox(&mut st.named_only, tl!("Named only"));
    });
    ui.separator();

    let query = st.query.trim().to_lowercase();
    let filtered: Vec<&Value> = regions
        .iter()
        .filter(|r| {
            let text = str(r, "text").unwrap_or("");
            let name = str(r, "name").unwrap_or("");
            let kind = str(r, "type").unwrap_or("");
            let orig = str(r, "key").and_then(|k| st.snapshot.get(k)).map(String::as_str).unwrap_or("");
            (query.is_empty() || name.to_lowercase().contains(&query) || kind.to_lowercase().contains(&query) || text.to_lowercase().contains(&query))
                && (!st.named_only || r["named"].as_bool().unwrap_or(false))
                && (!st.unfilled || text.trim().is_empty() || text == orig)
        })
        .collect();

    if regions.is_empty() {
        ui.label(egui::RichText::new(tl!("This document has no content controls or permission ranges.")).weak());
        app.regions = st;
        return;
    }
    ui.label(egui::RichText::new(tl!("Click a region to jump to it; double-click its name to rename it.")).small().weak());

    ScrollArea::vertical().id_salt("regions_list").show(ui, |ui| {
        egui::Grid::new("regions_grid").num_columns(6).spacing([8.0, 3.0]).striped(true).show(ui, |ui| {
            for head in ["#", "Name", "Type", "Original", "Modified", ""] {
                ui.label(egui::RichText::new(tl!(head)).weak());
            }
            ui.end_row();
            for (i, r) in filtered.iter().enumerate() {
                region_row(app, ui, &mut st, i + 1, r);
                ui.end_row();
            }
        });
    });

    // The editor sits below the list: one region edits at a time, so it doesn't need a row of
    // its own in the grid.
    let active = st.editing.as_deref().or(st.renaming.as_deref());
    if let Some(r) = active.and_then(|k| regions.iter().find(|r| str(r, "key") == Some(k))) {
        ui.separator();
        editor(app, ui, &mut st, r);
    }

    if !st.message.is_empty() {
        ui.separator();
        ui.label(egui::RichText::new(&st.message).small().color(ui.visuals().error_fg_color));
    }
    app.regions = st;
}

fn region_row(app: &mut WordApp, ui: &mut Ui, st: &mut RegionsPane, n: usize, r: &Value) {
    let key = str(r, "key").unwrap_or("").to_string();
    let text = str(r, "text").unwrap_or("").to_string();
    let original = st.snapshot.get(&key).map(String::as_str).unwrap_or("").to_string();
    let changed = text != original && !text.trim().is_empty();
    let locked = r["lockEdit"].as_bool().unwrap_or(false);

    ui.label(n.to_string());
    let name = str(r, "name").unwrap_or("").to_string();
    let name = ui.add(egui::Button::selectable(st.editing.as_deref() == Some(&key), egui::RichText::new(&name)).frame(false));
    if name.clicked() {
        jump(app, r);
    }
    if name.double_clicked() {
        st.renaming = Some(key.clone());
        st.editing = None;
        st.buffer = name_text(r);
    }
    ui.label(egui::RichText::new(str(r, "type").unwrap_or("")).weak());
    ui.label(egui::RichText::new(if original.trim().is_empty() { tl!("(blank)").to_string() } else { original.clone() }).weak().small());
    ui.label(egui::RichText::new(if changed { text.clone() } else { "—".to_string() }).small());
    ui.horizontal(|ui| {
        if ui.add_enabled(!locked, egui::Button::new(tl!("Edit")).small()).clicked() {
            st.editing = Some(key.clone());
            st.renaming = None;
            st.buffer = text.clone();
            st.message.clear();
        }
        if ui.add_enabled(changed || text.trim().is_empty(), egui::Button::new(tl!("Restore")).small()).clicked() {
            apply(app, st, r, &original);
        }
    });
}

/// The text box for the region being edited (or renamed), with OK and Cancel.
fn editor(app: &mut WordApp, ui: &mut Ui, st: &mut RegionsPane, r: &Value) {
    let renaming = st.renaming.is_some();
    let head = if renaming {
        tl!("Name for this region:").to_string()
    } else {
        crate::i18n::fmt(tl!("Content of {name}:"), &[("name", str(r, "name").unwrap_or(""))])
    };
    ui.label(egui::RichText::new(head));
    ui.horizontal(|ui| {
        let commit =
            ui.add(egui::TextEdit::singleline(&mut st.buffer).desired_width(200.0)).lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(tl!("OK")).clicked() || commit {
            if renaming {
                rename(app, st, r);
            } else {
                let text = st.buffer.clone();
                apply(app, st, r, &text);
            }
        }
        if ui.button(tl!("Cancel")).clicked() {
            st.editing = None;
            st.renaming = None;
        }
    });
}

fn apply(app: &mut WordApp, st: &mut RegionsPane, r: &Value, text: &str) {
    let (Some(a), Some(b)) = (r.get("start"), r.get("end")) else {
        return;
    };
    if let Err(e) = app.run("select.range", json!({"anchor": a, "focus": b})) {
        st.message = e;
        return;
    }
    // Raw: the pane sets content, it doesn't type prose for AutoCorrect to improve.
    if let Err(e) = app.run("text.insert", json!({"text": text, "raw": true})) {
        st.message = e;
        return;
    }
    st.editing = None;
    st.message.clear();
}

fn rename(app: &mut WordApp, st: &mut RegionsPane, r: &Value) {
    let Some(at) = r.get("start") else { return };
    let title = st.buffer.trim().to_string();
    let p = if title.is_empty() { json!({"at": at}) } else { json!({"at": at, "title": title}) };
    match app.run("developer.properties", p) {
        Ok(_) => {
            st.renaming = None;
            st.message.clear();
        }
        Err(e) => st.message = e,
    }
}

fn jump(app: &mut WordApp, r: &Value) {
    let (Some(a), Some(b)) = (r.get("start"), r.get("end")) else { return };
    let _ = app.run("select.range", json!({"anchor": a, "focus": b}));
    // Align the region against this pane, as the Navigation pane does for its headings.
    app.canvas.nav_to_caret = true;
    app.canvas.scroll_to_caret = false;
    app.canvas.want_focus = true;
}

fn name_text(r: &Value) -> String {
    str(r, "name").unwrap_or("").to_string()
}

fn str<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::apply;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;
    use wordcraft_doc::Document;

    use crate::{Services, WordApp};

    fn app() -> WordApp {
        let mut a = WordApp::new(wordcraft_engine::Session::new(Document::from_text("Hello\nWorld\n")), Services::default());
        // A named control on the first line, an everyone permission range on the second.
        a.run("select.range", json!({"anchor": {"block": 0, "off": 0}, "focus": {"block": 0, "off": 5}})).unwrap();
        a.run("developer.richText", json!({"title": "Greeting"})).unwrap();
        a.run("select.range", json!({"anchor": {"block": 1, "off": 0}, "focus": {"block": 1, "off": 5}})).unwrap();
        a.run("review.restrict", json!({"exception": "add"})).unwrap();
        a
    }

    fn harness(a: WordApp) -> egui_kittest::Harness<'static, WordApp> {
        egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            a,
        )
    }

    #[test]
    fn the_pane_lists_and_jumps_to_regions() {
        let mut a = app();
        a.run("ui.regionsPane", json!({})).unwrap();
        assert!(a.regions.open, "no value toggles the pane");
        let list = a.session.run("review.regions", &json!({})).unwrap();
        assert_eq!(list["count"], json!(2), "{list:?}");

        let mut h = harness(a);
        h.run_steps(4);
        assert!(h.query_by_label("Greeting").is_some(), "the control's row shows its name");
        assert!(h.query_by_label("Everyone").is_some(), "the permission range's row shows who may edit");
        // Clicking a row jumps: the selection moves into that region.
        h.get_by_label("Greeting").click();
        h.run_steps(4);
        let sel = h.state().session.sel.ordered();
        assert_eq!(sel.0.path.0, vec![0], "the jump lands on the control's paragraph");
    }

    #[test]
    fn the_row_buttons_open_and_close_the_inline_editor() {
        // One control only, so the row's Edit button is the only one.
        let mut a = app();
        a.run("select.range", json!({"anchor": {"block": 1, "off": 0}, "focus": {"block": 1, "off": 5}})).unwrap();
        a.run("review.restrict", json!({"exception": "remove"})).unwrap();
        a.run("ui.regionsPane", json!({"value": true})).unwrap();

        let mut h = harness(a);
        h.run_steps(4);
        h.get_by_label("Edit").click();
        h.run_steps(2);
        assert!(h.state().regions.editing.is_some(), "Edit opens the inline box");
        h.get_by_label("Cancel").click();
        h.run_steps(2);
        assert!(h.state().regions.editing.is_none(), "Cancel closes it");
    }

    #[test]
    fn a_row_edit_replaces_the_region_content() {
        let mut a = app();
        a.run("select.range", json!({"anchor": {"block": 1, "off": 0}, "focus": {"block": 1, "off": 5}})).unwrap();
        a.run("review.restrict", json!({"exception": "remove"})).unwrap();
        a.run("ui.regionsPane", json!({"value": true})).unwrap();
        let region = a.session.run("review.regions", &json!({})).unwrap()["regions"][0].clone();

        let mut st = std::mem::take(&mut a.regions);
        apply(&mut a, &mut st, &region, "Replaced");
        let v = a.session.run("review.regions", &json!({})).unwrap();
        assert_eq!(v["regions"][0]["text"], "Replaced", "{v:?}");
        // The pane re-queries every frame, so the second edit uses the fresh range.
        let region = v["regions"][0].clone();
        apply(&mut a, &mut st, &region, "");
        a.regions = st;
        let v = a.session.run("review.regions", &json!({})).unwrap();
        assert_eq!(v["regions"][0]["text"], "", "{v:?}");
    }

    #[test]
    fn an_empty_document_says_so() {
        let mut a = WordApp::new(wordcraft_engine::Session::new(Document::from_text("Plain prose\n")), Services::default());
        a.run("ui.regionsPane", json!({"value": true})).unwrap();
        let mut h = harness(a);
        h.run_steps(4);
        assert!(h.query_by_label("This document has no content controls or permission ranges.").is_some(), "the empty note shows",);
    }
}
