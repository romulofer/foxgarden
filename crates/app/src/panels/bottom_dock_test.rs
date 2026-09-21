
use super::*;

#[test]
fn a_fresh_dock_is_closed() {
    let dock = BottomDock::default();
    assert!(!dock.is_open());
    assert!(!dock.shows(BottomTab::Terminal));
}

#[test]
fn toggling_a_closed_dock_opens_it_on_that_tab() {
    let mut dock = BottomDock::default();
    dock.toggle(BottomTab::Build);
    assert!(dock.shows(BottomTab::Build));
    assert!(!dock.shows(BottomTab::Terminal));
}

#[test]
fn toggling_the_tab_already_showing_closes_the_dock() {
    let mut dock = BottomDock::default();
    dock.toggle(BottomTab::Terminal);
    dock.toggle(BottomTab::Terminal);
    assert!(!dock.is_open());
}

#[test]
fn toggling_a_different_tab_switches_instead_of_closing() {
    let mut dock = BottomDock::default();
    dock.open_tab(BottomTab::Build);
    dock.toggle(BottomTab::Terminal);
    assert!(dock.shows(BottomTab::Terminal));
    assert!(!dock.shows(BottomTab::Build));
}

#[test]
fn reopening_lands_on_the_last_tab_shown() {
    let mut dock = BottomDock::default();
    dock.open_tab(BottomTab::Profiler);
    dock.close();
    dock.toggle(BottomTab::Profiler);
    assert!(dock.shows(BottomTab::Profiler));
}

#[test]
fn open_tab_never_closes_the_dock() {
    let mut dock = BottomDock::default();
    dock.open_tab(BottomTab::Build);
    dock.open_tab(BottomTab::Build);
    assert!(dock.shows(BottomTab::Build));
}

#[test]
fn every_tab_round_trips_through_its_persistence_key() {
    for tab in BottomTab::ALL {
        assert_eq!(BottomTab::from_key(tab.key()), Some(tab));
    }
    assert_eq!(BottomTab::from_key("source_control"), None);
}
