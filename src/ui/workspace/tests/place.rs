use super::*;

fn display(id: u32, name: &str) -> DisplayInfo {
    DisplayInfo {
        id,
        name: name.to_owned(),
        bounds: Bounds::default(),
    }
}

fn place(display: u32, name: Option<&str>) -> Place {
    Place {
        display,
        name: name.map(str::to_owned),
        x: 10.,
        y: 20.,
        width: 800.,
        height: 600.,
    }
}

#[test]
fn finds_the_display_by_its_id_or_else_its_name() {
    let connected = [display(1, "Built-in"), display(5, "Studio")];
    assert_eq!(place(5, Some("Other")).find(&connected), Some(5), "by id");
    assert_eq!(
        place(9, Some("Studio")).find(&connected),
        Some(5),
        "by name"
    );
    assert_eq!(place(9, Some("Gone")).find(&connected), None);
    assert_eq!(place(9, None).find(&connected), None);

    let twins = [display(2, "Studio"), display(3, "Studio")];
    assert_eq!(
        place(9, Some("Studio")).find(&twins),
        None,
        "not one of two of the same name"
    );
}

#[test]
fn a_place_saves_and_loads_as_it_was() {
    let saved = Place {
        name: Some("Studio".into()),
        ..place(5, None)
    };
    let text = serde_json::to_string(&saved).unwrap();
    assert_eq!(serde_json::from_str::<Place>(&text).unwrap(), saved);
    assert_eq!(saved.bounds().origin, point(px(10.), px(20.)));
}
