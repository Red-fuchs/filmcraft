//! Stretching a marker's range on the Timeline ruler (#1348): a drag on the end of a range band
//! changes its duration, Option-drag on a pentagon gives a point marker a range, a plain drag on a
//! pentagon still scrubs, and one undo puts the duration back.

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
fn option_drag_on_a_point_marker_gives_it_a_range_and_a_plain_drag_scrubs() {
    let mut d = Driver::demo();
    d.exec("markers.clearAll", json!({}));
    let id = d.exec("markers.add", json!({"frame": 30, "name": "Point"}))["marker"].as_u64().unwrap();
    d.frames(2);
    let r = d.rect(&format!("timeline.marker.{id}"));
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    d.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": x + 150.0, "y": y}, "steps": 10}));
    d.frames(3);
    assert_eq!(marker(&d, id).duration, filmcraft_time::Tick::ZERO, "a plain drag does not stretch");
    d.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": x + 150.0, "y": y}, "steps": 10, "modifiers": {"alt": true}}));
    d.frames(3);
    let m = marker(&d, id);
    assert!(m.duration > filmcraft_time::Tick::ZERO, "Option-drag made a range");
    assert_eq!(m.name, "Point");
}
