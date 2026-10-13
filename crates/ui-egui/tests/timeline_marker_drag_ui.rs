//! Stretching a marker's range on the Timeline ruler (#1348): a drag on the end of a range band
//! changes its duration, Option-drag on a pentagon gives a point marker a range, a plain drag on a
//! pentagon or band moves the marker, and one undo puts it back.

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

struct Driver {
    harness: Harness<'static, FilmcraftApp>,
    tx: Sender<ControlRequest>,
}

impl Driver {
    fn demo() -> Self {
        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).expect("demo project");
        let (tx, rx) = channel();
        let app = FilmcraftApp::new(session).with_control(rx);
        let harness = Harness::builder().with_size(egui::vec2(1600.0, 980.0)).with_max_steps(10_000).build_eframe(move |_cc| app);
        let mut d = Driver { harness, tx };
        d.frames(4);
        d
    }
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let ctx = self.harness.ctx.clone();
            let mut raw = std::mem::take(self.harness.input_mut());
            eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
            *self.harness.input_mut() = raw;
            self.harness.step();
        }
    }
    fn ok(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                assert_eq!(v["ok"], json!(true), "{method} {params} failed: {v}");
                return v["result"].clone();
            }
        }
        panic!("no reply to {method} {params}");
    }
    fn exec(&mut self, command: &str, params: Value) -> Value {
        self.ok("engine.execute", json!({"command": command, "params": params}))
    }
    fn rect(&mut self, id: &str) -> [f64; 4] {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        let e = v.as_array().unwrap().iter().find(|e| e["id"] == id).unwrap_or_else(|| panic!("no element {id}")).clone();
        let r: Vec<f64> = e["rect"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        [r[0], r[1], r[2], r[3]]
    }
}

fn marker(d: &Driver, id: u64) -> filmcraft_project::Marker {
    d.harness.state().session.active_sequence().unwrap().markers.iter().find(|m| m.id.0 == id).unwrap().clone()
}

#[test]
fn dragging_a_range_end_changes_the_duration_and_undo_restores_it() {
    let mut d = Driver::demo();
    d.exec("markers.clearAll", json!({}));
    let id = d.exec("markers.add", json!({"frame": 30, "durationFrames": 30, "name": "Range"}))["marker"].as_u64().unwrap();
    d.frames(2);
    let before = marker(&d, id);
    let band = d.rect(&format!("timeline.markerRange.{id}"));
    let (x, y) = (band[0] + band[2], band[1] + band[3] / 2.0);
    d.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": x + 120.0, "y": y}, "steps": 10}));
    d.frames(3);
    let after = marker(&d, id);
    assert_eq!(after.start, before.start, "the start stays");
    assert!(after.duration > before.duration, "the range grew: {:?} -> {:?}", before.duration, after.duration);
    let rate = d.harness.state().session.sequence_rate();
    assert_eq!(rate.snap_nearest(after.duration), after.duration, "the duration lands on a frame");
    assert_eq!(after.name, "Range");
    d.exec("edit.undo", json!({}));
    d.frames(2);
    assert_eq!(marker(&d, id).duration, before.duration, "one undo step");

    // dragging the end to the left of the start leaves a point marker, never a negative range
    let band = d.rect(&format!("timeline.markerRange.{id}"));
    let (x, y) = (band[0] + band[2], band[1] + band[3] / 2.0);
    d.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": band[0] - 200.0, "y": y}, "steps": 10}));
    d.frames(3);
    let m = marker(&d, id);
    assert_eq!(m.duration, filmcraft_time::Tick::ZERO);
    assert_eq!(m.start, before.start);
}

#[test]
fn option_drag_on_a_point_marker_gives_it_a_range() {
    let mut d = Driver::demo();
    d.exec("markers.clearAll", json!({}));
    let id = d.exec("markers.add", json!({"frame": 30, "name": "Point"}))["marker"].as_u64().unwrap();
    d.frames(2);
    let before = marker(&d, id);
    let r = d.rect(&format!("timeline.marker.{id}"));
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    d.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": x + 150.0, "y": y}, "steps": 10, "modifiers": {"alt": true}}));
    d.frames(3);
    let m = marker(&d, id);
    assert!(m.duration > filmcraft_time::Tick::ZERO, "Option-drag made a range");
    assert_eq!(m.start, before.start, "the start stays");
    assert_eq!(m.name, "Point");
}

#[test]
fn a_plain_drag_on_a_pentagon_or_band_moves_the_marker() {
    let mut d = Driver::demo();
    d.exec("markers.clearAll", json!({}));
    let id = d.exec("markers.add", json!({"frame": 30, "durationFrames": 48, "name": "Move"}))["marker"].as_u64().unwrap();
    d.frames(2);
    let before = marker(&d, id);
    let rate = d.harness.state().session.sequence_rate();

    // by the pentagon: later, same duration, playhead on the new start, one undo step
    let r = d.rect(&format!("timeline.marker.{id}"));
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    d.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": x + 150.0, "y": y}, "steps": 10}));
    d.frames(3);
    let m = marker(&d, id);
    assert!(m.start > before.start, "moved right: {:?} -> {:?}", before.start, m.start);
    assert_eq!(rate.snap_nearest(m.start), m.start, "lands on a frame");
    assert_eq!(m.duration, before.duration, "a move keeps the duration");
    assert_eq!(d.harness.state().session.playhead(), m.start, "the playhead follows the marker");
    d.exec("edit.undo", json!({}));
    d.frames(2);
    assert_eq!(marker(&d, id).start, before.start, "one undo step");

    // by the middle of its band; dragged far left it stops at zero
    let band = d.rect(&format!("timeline.markerRange.{id}"));
    let (x, y) = (band[0] + band[2] / 2.0, band[1] + band[3] / 2.0);
    d.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": x - 2000.0, "y": y}, "steps": 10}));
    d.frames(3);
    let m = marker(&d, id);
    assert_eq!(m.start, filmcraft_time::Tick::ZERO);
    assert_eq!(m.duration, before.duration);
}
