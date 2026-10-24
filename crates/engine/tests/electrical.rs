//! Electrical identity, physical-device counting, persistence and failure atomicity.

use cadcraft_doc::{EntityKind, Handle, RawTag, Space};
use cadcraft_engine::Session;
use serde_json::json;

const ASSIGN: &str = "electrical.component.assign";
const UPDATE: &str = "electrical.component.update";
const DETACH: &str = "electrical.component.detach";
const LIST: &str = "electrical.components";
const BOM: &str = "electrical.bom";

fn drawing() -> (Session, Vec<String>) {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.execute("attdef", &json!({"tag": "TAG", "default": "VISIBLE", "at": [0, 2]})).unwrap();
    let entities: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "SYMBOL", "handles": entities, "base": [0, 0], "keep": "delete"})).unwrap();
    let handles = (0..5)
        .map(|x| {
            s.execute("insert", &json!({"name": "SYMBOL", "at": [x * 5, 0], "attribs": {"TAG": "VISIBLE"}})).unwrap()["handle"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    (s, handles)
}

fn primary(s: &mut Session, handle: &str, tag: &str) -> String {
    s.execute(
        ASSIGN,
        &json!({"handle": handle, "tag": tag, "role": "coil", "family": "relay",
        "manufacturer": "Example", "catalog": "R-2", "description": "Control relay", "location": "Panel A"}),
    )
    .unwrap()["device"]
        .as_str()
        .unwrap()
        .to_string()
}

fn relay() -> (Session, Vec<String>, String) {
    let (mut s, hs) = drawing();
    let id = primary(&mut s, &hs[0], "K1");
    for h in &hs[1..3] {
        s.execute(ASSIGN, &json!({"handle": h, "device": id, "role": "contact"})).unwrap();
    }
    (s, hs, id)
}

fn quantity(s: &mut Session) -> u64 {
    s.execute(BOM, &json!({})).unwrap()["rows"].as_array().unwrap().iter().map(|r| r[0].as_u64().unwrap()).sum()
}

#[test]
fn coil_and_contacts_count_once_and_separate_relays_count_twice() {
    let (mut s, hs, id) = relay();
    assert_eq!(id.len(), 32);
    let one = s.execute(BOM, &json!({})).unwrap();
    assert_eq!(one["rows"], json!([[1, "Example", "R-2", "Control relay", "relay", "", "Panel A", "K1"]]));
    assert_eq!(one["devices"], json!([[id]]));
    assert_eq!(one["unassigned"], json!([hs[3], hs[4]]));
    let second = primary(&mut s, &hs[3], "K2");
    assert_ne!(id, second);
    let two = s.execute(BOM, &json!({})).unwrap();
    assert_eq!(two["rows"][0][0], 2);
    assert_eq!(two["rows"][0][7], "K1; K2");
    assert_eq!(two["devices"][0].as_array().unwrap().len(), 2);
}

#[test]
fn shared_properties_update_atomically_and_queries_are_read_only() {
    let (mut s, _, id) = relay();
    let before = s.doc().unwrap().clone();
    let undo = s.state().unwrap().undo.len();
    let revision = s.state().unwrap().revision;
    for cmd in [BOM, LIST] {
        s.execute(cmd, &json!({})).unwrap();
    }
    assert_eq!(*s.doc().unwrap(), before);
    assert_eq!(s.state().unwrap().undo.len(), undo);
    assert_eq!(s.state().unwrap().revision, revision);
    s.execute(UPDATE, &json!({"device": id, "tag": "K9", "catalog": "R-4", "description": "日本語 relay"})).unwrap();
    let updated = s.execute(LIST, &json!({})).unwrap();
    assert_eq!(updated["devices"][0]["device"], id);
    assert_eq!(updated["devices"][0]["properties"]["tag"], "K9");
    assert_eq!(updated["devices"][0]["representations"].as_array().unwrap().len(), 3);
    s.execute("u", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap(), before);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(s.execute(LIST, &json!({})).unwrap(), updated);
    let undo = s.state().unwrap().undo.len();
    s.execute(UPDATE, &json!({"device": id, "tag": "K9"})).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo, "no-op edit has no undo step");
}

#[test]
fn detach_and_delete_keep_counting_live_devices_and_undo_restores_them() {
    let (mut s, hs, id) = relay();
    s.execute(DETACH, &json!({"handle": hs[0]})).unwrap();
    assert_eq!(quantity(&mut s), 1, "contact views still represent the physical relay");
    assert_eq!(s.execute(BOM, &json!({})).unwrap()["missingPrimary"], json!([id]));
    for h in &hs[1..3] {
        s.execute("erase", &json!({"handles": [h]})).unwrap();
    }
    assert_eq!(quantity(&mut s), 0);
    s.execute("u", &json!({})).unwrap();
    assert_eq!(quantity(&mut s), 1);
    s.execute(DETACH, &json!({"handle": hs[2]})).unwrap();
    assert_eq!(quantity(&mut s), 0);
}

#[test]
fn generic_copy_is_unassigned_and_can_become_a_new_device() {
    let (mut s, hs, original) = relay();
    let copied = s.execute("copy", &json!({"handles": [hs[0]], "from": [0, 0], "to": [10, 10]})).unwrap();
    let copy = copied["handles"][0].as_str().unwrap();
    assert_eq!(quantity(&mut s), 1);
    let list = s.execute(LIST, &json!({})).unwrap();
    assert!(list["unassigned"].as_array().unwrap().contains(&json!(copy)));
    let new = primary(&mut s, copy, "K2");
    assert_ne!(original, new);
    assert_eq!(quantity(&mut s), 2);
    let bytes = cadcraft_io::write(s.doc().unwrap(), "copy.dxf").unwrap();
    let d = cadcraft_io::read(&bytes, "copy.dxf").unwrap();
    assert_eq!(d.electrical.devices.len(), 2);
}

#[test]
fn scopes_and_primary_roles_prevent_ambiguous_devices() {
    let (mut s, hs, id) = relay();
    let before = s.doc().unwrap().clone();
    assert!(s.execute(ASSIGN, &json!({"handle": hs[3], "tag": "k1", "location": "panel a"})).is_err());
    assert!(s.execute(ASSIGN, &json!({"handle": hs[3], "device": id})).is_err());
    assert!(s.execute(ASSIGN, &json!({"handle": hs[3], "device": id, "role": "contact", "tag": "K2"})).is_err());
    assert_eq!(*s.doc().unwrap(), before);
    s.execute(ASSIGN, &json!({"handle": hs[3], "tag": "K1", "location": "Panel B"})).unwrap();
    assert_eq!(quantity(&mut s), 2);
    assert_eq!(s.execute(BOM, &json!({})).unwrap()["missingCatalog"].as_array().unwrap().len(), 1);
}

#[test]
fn malformed_parameters_never_mutate_or_panic() {
    let (mut s, hs, id) = relay();
    let mut cases = vec![
        (ASSIGN, json!(null)),
        (ASSIGN, json!([])),
        (ASSIGN, json!({"handle": 5})),
        (ASSIGN, json!({"handle": "FFFFFFFFFFFFFFFFF", "tag": "K2"})),
        (ASSIGN, json!({"handle": hs[3], "tag": ""})),
        (ASSIGN, json!({"handle": hs[3], "tag": "K2", "role": "bogus"})),
        (ASSIGN, json!({"handle": hs[3], "tag": "K2", "role": "contact"})),
        (ASSIGN, json!({"handle": hs[3], "device": null})),
        (ASSIGN, json!({"handle": hs[3], "tag": "K2", "manufacturer": false})),
        (UPDATE, json!({"device": id, "description": "x".repeat(1025)})),
        (UPDATE, json!({"device": id, "tag": "bad\nfield"})),
        (UPDATE, json!({"device": id, "catalog": null})),
        (UPDATE, json!({"device": "missing", "tag": "K3"})),
        (DETACH, json!({"handle": hs[3]})),
        (LIST, json!({"typo": true})),
        (BOM, json!({"file": false})),
        (BOM, json!({"file": "bad.txt"})),
    ];
    s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap();
    let line = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
    cases.push((ASSIGN, json!({"handle": line.hex(), "tag": "L1"})));
    let before = s.doc().unwrap().clone();
    let undo = s.state().unwrap().undo.len();
    for (cmd, params) in cases {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.execute(cmd, &params)));
        assert!(result.is_ok(), "{cmd} panicked for {params}");
        assert!(result.unwrap().is_err(), "{cmd} accepted {params}");
        assert_eq!(*s.doc().unwrap(), before, "{cmd} changed a rejected drawing");
        assert_eq!(s.state().unwrap().undo.len(), undo);
    }
}

#[test]
fn dxf_ascii_binary_and_dwg_preserve_identity_shared_fields_and_foreign_xdata() {
    let (mut s, hs, id) = relay();
    let text = "日本語 abc ".repeat(60);
    s.execute(UPDATE, &json!({"device": id, "description": text})).unwrap();
    let h = Handle::parse_hex(&hs[0]).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.xdata = vec![RawTag { code: 1001, value: "EXAMPLE_FOREIGN".into() }, RawTag { code: 1000, value: "preserve me".into() }];
        })
        .unwrap();
    let expected = s.execute(LIST, &json!({})).unwrap();
    for (name, binary) in [("test.dxf", false), ("test.dxf", true), ("test.dwg", false)] {
        let bytes = cadcraft_io::write_with(s.doc().unwrap(), name, &cadcraft_io::SaveOptions { binary, ..Default::default() }).unwrap();
        let read = cadcraft_io::read(&bytes, name).unwrap();
        assert_eq!(read.electrical.devices[&id].description, text.trim(), "{name}/{binary}");
        assert_eq!(read.electrical.representations.len(), 3, "{name}/{binary}");
        assert!(read.model.iter().any(|e| e.common.xdata.iter().any(|t| t.code == 1000 && t.value == "preserve me")));
        let bytes = cadcraft_io::write(&read, name).unwrap();
        let again = cadcraft_io::read(&bytes, name).unwrap();
        let mut reopened = Session::new();
        reopened.open_drawing(again, name, None);
        let actual = reopened.execute(LIST, &json!({})).unwrap();
        // DXF preserves handles; DWG is allowed to renumber them. Identity and count are stable.
        assert_eq!(actual["devices"][0]["device"], expected["devices"][0]["device"]);
        assert_eq!(actual["devices"][0]["properties"], expected["devices"][0]["properties"]);
        assert_eq!(quantity(&mut reopened), 1);
    }
}

#[test]
fn paper_space_counts_and_multiple_insert_arrays_are_rejected() {
    let (mut s, hs) = drawing();
    let h = Handle::parse_hex(&hs[0]).unwrap();
    let e = s.doc_mut().unwrap().remove_entity(h).unwrap();
    s.doc_mut().unwrap().layouts[0].entities.push((*e).clone());
    primary(&mut s, &hs[0], "K1");
    assert_eq!(quantity(&mut s), 1);
    assert_eq!(s.doc().unwrap().space_of(h), Some(Space::Paper("Layout1".into())));
    let h = Handle::parse_hex(&hs[1]).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.cols = 2;
            }
        })
        .unwrap();
    assert!(s.execute(ASSIGN, &json!({"handle": hs[1], "tag": "K2"})).is_err());
    let bytes = cadcraft_io::write(s.doc().unwrap(), "paper.dxf").unwrap();
    assert_eq!(cadcraft_io::read(&bytes, "paper.dxf").unwrap().electrical.devices.len(), 1);
}

#[test]
fn csv_is_escaped_and_export_does_not_change_drawing() {
    let (mut s, _, id) = relay();
    s.execute(UPDATE, &json!({"device": id, "description": "Relay, \"two contacts\"", "manufacturer": "=formula"})).unwrap();
    let file = std::env::temp_dir().join(format!("cadcraft-electrical-bom-{}.csv", std::process::id()));
    let before = s.doc().unwrap().clone();
    let undo = s.state().unwrap().undo.len();
    s.execute(BOM, &json!({"file": file})).unwrap();
    let csv = std::fs::read_to_string(&file).unwrap();
    assert!(csv.starts_with("Quantity,Manufacturer,Catalog,Description,Family,Installation,Location,Tags\r\n"));
    assert!(csv.contains("'=formula"));
    assert!(csv.contains("\"Relay, \"\"two contacts\"\"\""));
    assert_eq!(*s.doc().unwrap(), before);
    assert_eq!(s.state().unwrap().undo.len(), undo);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn raw_json_command_line_uses_the_same_commands() {
    let (mut s, hs) = drawing();
    s.cmdline(&format!("{ASSIGN} {}", json!({"handle": hs[0], "tag": "K1"}))).unwrap();
    assert_eq!(quantity(&mut s), 1);
    s.cmdline("electrical.bom {}").unwrap();
    assert!(s.running.is_none(), "JSON calls never start prompts");
}

#[test]
fn loading_then_copying_does_not_reimport_a_stale_device_binding() {
    let (s, _, _) = relay();
    let bytes = cadcraft_io::write(s.doc().unwrap(), "source.dxf").unwrap();
    let read = cadcraft_io::read(&bytes, "source.dxf").unwrap();
    assert!(read.model.iter().all(|e| !e.common.xdata.iter().any(|t| t.code == 1001 && t.value == cadcraft_doc::electrical::APP)));
    let mut reopened = Session::new();
    reopened.open_drawing(read, "source.dxf", None);
    let h = reopened.execute(LIST, &json!({})).unwrap()["devices"][0]["representations"][0]["handle"].as_str().unwrap().to_string();
    reopened.execute("copy", &json!({"handles": [h], "delta": [10, 0]})).unwrap();
    let bytes = cadcraft_io::write(reopened.doc().unwrap(), "copy.dxf").unwrap();
    let read = cadcraft_io::read(&bytes, "copy.dxf").unwrap();
    assert_eq!(read.electrical.representations.len(), 3);
    assert_eq!(read.electrical.devices.len(), 1);
}

#[test]
fn future_or_malformed_electrical_data_is_preserved_and_not_overwritten() {
    let (mut s, hs) = drawing();
    let h = Handle::parse_hex(&hs[0]).unwrap();
    let raw = vec![
        RawTag { code: 1001, value: cadcraft_doc::electrical::APP.into() },
        RawTag { code: 1070, value: "999".into() },
        RawTag { code: 1000, value: "future data".into() },
    ];
    s.doc_mut().unwrap().modify_entity(h, |e| e.common.xdata = raw.clone()).unwrap();
    let bytes = cadcraft_io::write(s.doc().unwrap(), "future.dxf").unwrap();
    let read = cadcraft_io::read(&bytes, "future.dxf").unwrap();
    assert!(read.electrical.devices.is_empty());
    assert_eq!(read.entity(h).unwrap().common.xdata, raw);
    assert!(read.read_notes.iter().any(|n| n.contains("kept uninterpreted")));
    s.open_drawing(read, "future.dxf", None);
    let before = s.doc().unwrap().clone();
    assert!(s.execute(ASSIGN, &json!({"handle": hs[0], "tag": "K1"})).is_err());
    assert_eq!(*s.doc().unwrap(), before);
    let bytes = cadcraft_io::write(s.doc().unwrap(), "future.dxf").unwrap();
    assert_eq!(cadcraft_io::read(&bytes, "future.dxf").unwrap().entity(h).unwrap().common.xdata, raw);
}

#[test]
fn conflicting_repeated_device_properties_are_rejected_on_import() {
    let (s, _, _) = relay();
    let bytes = cadcraft_io::write(s.doc().unwrap(), "relay.dxf").unwrap();
    let mut tags = cadcraft_dxf::parse(&bytes).unwrap();
    let description = tags.iter_mut().find(|t| t.code == 1000 && t.str() == "Control relay").unwrap();
    *description = cadcraft_dxf::Tag::s(1000, "Conflicting relay");
    let text = cadcraft_dxf::write_ascii_as(&tags, true);
    let error = cadcraft_io::read(text.as_bytes(), "relay.dxf").unwrap_err();
    assert!(error.to_string().contains("conflicting electrical properties"));
}

#[test]
fn locked_layers_protect_all_representations_before_shared_edits() {
    let (mut s, hs, id) = relay();
    let h = Handle::parse_hex(&hs[1]).unwrap();
    s.doc_mut().unwrap().ensure_layer("LOCKED");
    s.doc_mut().unwrap().modify_entity(h, |e| e.common.layer = "LOCKED".into()).unwrap();
    s.doc_mut().unwrap().layers.iter_mut().find(|l| l.name == "LOCKED").unwrap().locked = true;
    let before = s.doc().unwrap().clone();
    assert!(s.execute(UPDATE, &json!({"device": id, "description": "Changed"})).is_err());
    assert!(s.execute(DETACH, &json!({"handle": hs[1]})).is_err());
    assert!(s.execute(ASSIGN, &json!({"handle": hs[1], "tag": "K2"})).is_err());
    assert_eq!(*s.doc().unwrap(), before);
}
