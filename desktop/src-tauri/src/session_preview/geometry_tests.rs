use super::*;

fn rect(x: f64, y: f64, width: f64, height: f64) -> SlotRect {
    SlotRect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn slot_is_inset_two_points_on_every_edge() {
    assert_eq!(
        view_bounds(rect(300.0, 40.0, 640.0, 480.0)),
        Ok(Placement::Show(rect(302.0, 42.0, 636.0, 476.0)))
    );
}

#[test]
fn fractional_rects_pass_through_unrounded() {
    // Logical points: AppKit rounds to the backing scale itself.
    assert_eq!(
        view_bounds(rect(10.5, 20.25, 100.0, 50.0)),
        Ok(Placement::Show(rect(12.5, 22.25, 96.0, 46.0)))
    );
}

#[test]
fn tiny_slots_collapse_instead_of_drawing_a_sliver() {
    assert_eq!(
        view_bounds(rect(0.0, 0.0, 11.0, 300.0)),
        Ok(Placement::Collapse)
    );
    assert_eq!(
        view_bounds(rect(0.0, 0.0, 0.0, 0.0)),
        Ok(Placement::Collapse)
    );
    assert!(matches!(
        view_bounds(rect(0.0, 0.0, 12.0, 12.0)),
        Ok(Placement::Show(_))
    ));
}

#[test]
fn impossible_rects_are_refused() {
    for bad in [
        rect(f64::NAN, 0.0, 10.0, 10.0),
        rect(0.0, f64::INFINITY, 10.0, 10.0),
        rect(0.0, 0.0, -1.0, 10.0),
        rect(0.0, 0.0, 10.0, -5.0),
        rect(200_000.0, 0.0, 10.0, 10.0),
    ] {
        assert!(view_bounds(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn rect_sequence_is_last_write_wins() {
    let mut seq = RectSequence::default();
    assert!(seq.accept(5));
    assert!(seq.accept(5), "a resend is idempotent");
    assert!(!seq.accept(4), "older is stale");
    assert!(seq.accept(9));
    assert!(!seq.accept(6));
    // An unmount is ordered too (no reset): a stale one is dropped.
    assert!(
        !seq.accept(8),
        "an unmount older than the last mount is stale"
    );
}
