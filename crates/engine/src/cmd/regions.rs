//! Editable Regions (`review.regions`): every content control and permission range in the document,
//! in document order — the data behind the Editable Regions pane. A "region" is anywhere the
//! document invites or confines input: a content control (`w:sdt`) or a range that stays editable
//! while the document is protected (`w:permStart`).

use serde_json::{Value, json};
use wordcraft_doc::{ControlKind, ControlRange, PermRange, Pos, StoryRef};

use crate::cmd::pos_json;
use crate::{CmdResult, CommandSpec, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("review.regions", "Editable Regions", "Review › Protect", regions).params(r#"{}"#).pure()]
}

fn regions(s: &mut Session, _v: &Value) -> CmdResult {
    let list = list(s);
    Ok(json!({"count": list.len(), "regions": list}))
}

/// One entry per region, in document order: content controls and permission ranges interleaved.
/// Permission ranges for other editors are left out: the pane is about regions `author` can edit.
pub fn list(s: &Session) -> Vec<Value> {
    let mut items: Vec<(usize, Pos, Value)> = Vec::new();
    let stories = std::iter::once(StoryRef::Body).chain(s.doc.parts.keys().map(|k| StoryRef::Part(*k)));
    for (i, story) in stories.enumerate() {
        for r in s.doc.control_ranges(story) {
            let at = r.content_start();
            items.push((i, at, control(s, &r)));
        }
        for r in s.doc.perm_ranges().into_iter().filter(|r| r.start.story == story && mine(r, &s.author)) {
            let at = r.start.clone();
            items.push((i, at, permission(s, &r)));
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.path.cmp(&b.1.path)).then(a.1.off.cmp(&b.1.off)));
    items.into_iter().map(|(_, _, v)| v).collect()
}

/// May `author` edit inside the range? (`protect::editable_by`, which is private to that module.)
fn mine(r: &PermRange, author: &str) -> bool {
    r.everyone() || (!r.editor.is_empty() && r.editor.eq_ignore_ascii_case(author.trim()))
}

fn control(s: &Session, r: &ControlRange) -> Value {
    let c = &r.control;
    let named = !c.title.trim().is_empty();
    let key = match c.id {
        Some(id) => format!("ctl:{id}"),
        None if !c.tag.is_empty() => format!("ctl:{}", c.tag),
        None => format!("ctl@{}", pos_json(&r.start)),
    };
    let text = if c.showing_placeholder { String::new() } else { s.doc.control_text(r) };
    json!({
        "kind": "control",
        "key": key,
        "name": if named { c.title.clone() } else { default_name(&c.kind).to_string() },
        "named": named,
        "type": c.kind.name(),
        "tag": c.tag,
        "start": pos_json(&r.content_start()),
        "end": pos_json(&r.end),
        "marker": pos_json(&r.start),
        "text": text,
        "showingPlaceholder": c.showing_placeholder,
        "lockEdit": !c.lock.can_edit(),
        "lockDelete": !c.lock.can_delete(),
        "singlePara": r.content_start().path == r.end.path,
    })
}

fn permission(s: &Session, r: &PermRange) -> Value {
    let name = if r.editor.is_empty() { "Everyone".to_string() } else { r.editor.clone() };
    let text = s.doc.copy_range(&r.start, &r.end).plain_text();
    json!({
        "kind": "permission",
        "key": format!("perm:{}", r.id),
        "name": name,
        "named": false,
        "type": "permission",
        "start": pos_json(&r.start),
        "end": pos_json(&r.end),
        "text": text,
        "singlePara": r.start.path == r.end.path,
    })
}

fn default_name(kind: &ControlKind) -> &'static str {
    match kind {
        ControlKind::RichText => "Rich Text Control",
        ControlKind::Text { .. } => "Plain Text Control",
        ControlKind::CheckBox { .. } => "Check Box Control",
        ControlKind::ComboBox { .. } => "Combo Box Control",
        ControlKind::DropDown { .. } => "Drop-Down Control",
        ControlKind::Date { .. } => "Date Control",
        ControlKind::Picture => "Picture Control",
        ControlKind::Gallery { .. } => "Building Block Control",
        ControlKind::RepeatingSection { .. } => "Repeating Section Control",
        ControlKind::RepeatingSectionItem => "Repeating Section Item",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wordcraft_doc::Document;

    use crate::Session;

    fn list_of(s: &mut Session) -> Vec<Value> {
        s.run("review.regions", &json!({})).unwrap()["regions"].as_array().unwrap().clone()
    }

    fn set_sel(s: &mut Session, block: u32, a: usize, b: usize) {
        s.run("select.range", &json!({"anchor": {"block": block, "off": a}, "focus": {"block": block, "off": b}})).unwrap();
    }

    #[test]
    fn lists_controls_and_permission_ranges_in_document_order() {
        let mut s = Session::new(Document::from_text("Hello\nWorld\n"));
        set_sel(&mut s, 0, 0, 5);
        s.run("developer.richText", &json!({"title": "Greeting"})).unwrap();
        set_sel(&mut s, 1, 0, 5);
        s.run("review.restrict", &json!({"exception": "add"})).unwrap();

        let rs = list_of(&mut s);
        assert_eq!(rs.len(), 2, "{rs:?}");
        assert_eq!(rs[0]["kind"], "control");
        assert_eq!(rs[0]["name"], "Greeting");
        assert_eq!(rs[0]["named"], json!(true));
        assert_eq!(rs[0]["text"], "Hello");
        assert_eq!(rs[1]["kind"], "permission");
        assert_eq!(rs[1]["name"], "Everyone");
        assert_eq!(rs[1]["text"], "World");
    }

    #[test]
    fn an_untitled_control_falls_back_to_its_kind_and_a_placeholder_reads_empty() {
        let mut s = Session::new(Document::from_text("Fill in: here\n"));
        set_sel(&mut s, 0, 9, 13);
        s.run("developer.plainText", &json!({})).unwrap();
        let rs = list_of(&mut s);
        assert_eq!(rs.len(), 1, "{rs:?}");
        assert_eq!(rs[0]["named"], json!(false));
        assert_eq!(rs[0]["name"], "Plain Text Control");
        // The control wraps the selected text, which is its content.
        assert_eq!(rs[0]["showingPlaceholder"], json!(false));
        assert_eq!(rs[0]["text"], "here");
    }

    #[test]
    fn ranges_for_other_editors_are_not_listed() {
        use wordcraft_doc::{Path, StoryRef};
        let at = |off| Pos::new(StoryRef::Body, Path(vec![0]), off);
        let bob = PermRange { id: 1, group: String::new(), editor: "bob".into(), start: at(0), end: at(4) };
        assert!(mine(&bob, "BOB"), "the named editor, case-insensitively");
        assert!(!mine(&bob, "alice"));
        let everyone = PermRange { id: 2, group: "everyone".into(), editor: String::new(), start: at(5), end: at(9) };
        assert!(mine(&everyone, "alice"));
    }

    #[test]
    fn junk_params_never_panic() {
        let mut s = Session::new(Document::from_text("x"));
        for v in [json!(null), json!({"kind": 3}), json!([1, 2]), json!("regions")] {
            let _ = s.run("review.regions", &v);
        }
    }
}
