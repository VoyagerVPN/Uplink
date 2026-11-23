use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{
    self, Clipboard, Key, Platform, PointerEventButton, WindowAdapter, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize};
use std::{cell::RefCell, rc::Rc};

slint::include_modules!();

struct Desktop(Rc<MinimalSoftwareWindow>);
impl Platform for Desktop {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
    fn clipboard_text(&self, _: Clipboard) -> Option<String> {
        Some("uplink://join/after-hours".into())
    }
}

fn open() -> (RoomWindow, Rc<MinimalSoftwareWindow>) {
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    platform::set_platform(Box::new(Desktop(surface.clone()))).unwrap();
    let ui = RoomWindow::new().unwrap();
    ui.set_nickname("Ranazy".into());
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    (ui, surface)
}

fn click(ui: &RoomWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    let button = PointerEventButton::Left;
    ui.window()
        .dispatch_event(WindowEvent::PointerPressed { position, button });
    ui.window()
        .dispatch_event(WindowEvent::PointerReleased { position, button });
    platform::update_timers_and_animations();
}

fn key(ui: &RoomWindow, text: impl Into<slint::SharedString>) {
    let text = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

#[test]
fn open_language_list_allows_the_first_click_on_settings_and_switches() {
    let (ui, _) = open();
    ui.set_settings_open(true);
    ui.set_languages(slint::ModelRc::new(slint::VecModel::from(vec![
        LanguageChoice {
            code: "en".into(),
            name: "English".into(),
        },
        LanguageChoice {
            code: "ru".into(),
            name: "Русский".into(),
        },
    ])));
    let events = Rc::new(RefCell::new(Vec::new()));
    let record = events.clone();
    ui.on_change_option(move |key, _| record.borrow_mut().push(key.to_string()));
    click(&ui, 292., 318.);
    click(&ui, 332., 133.);
    assert_eq!(*events.borrow(), ["autostart"]);
    click(&ui, 292., 318.);
    click(&ui, 281., 28.);
    assert!(!ui.get_settings_open());
}

#[test]
fn saved_networks_can_be_switched_from_the_main_screen() {
    let (ui, _) = open();
    ui.set_joined(true);
    ui.set_invite_url("uplink://join/after-hours".into());
    ui.set_rooms(slint::ModelRc::new(slint::VecModel::from(vec![
        SavedRoom {
            name: "After hours".into(),
            url: ui.get_invite_url(),
            ..Default::default()
        },
        SavedRoom {
            name: "Co-op".into(),
            url: "uplink://coop.example/rooms/coop".into(),
            ..Default::default()
        },
    ])));
    let selections = Rc::new(RefCell::new(Vec::new()));
    let record = selections.clone();
    ui.on_select_room(move |url| record.borrow_mut().push(url.to_string()));
    click(&ui, 37., 174.);
    assert_eq!(*selections.borrow(), ["uplink://coop.example/rooms/coop"]);
    let submitted = Rc::new(RefCell::new(String::new()));
    let result = submitted.clone();
    ui.on_join_room(move |url| *result.borrow_mut() = url.to_string());
    click(&ui, 37., 214.);
    assert!(ui.get_adding_network());
    click(&ui, 37., 174.);
    assert!(
        !ui.get_adding_network(),
        "Clicking the backdrop dismisses the join form"
    );
    assert_eq!(
        selections.borrow().len(),
        1,
        "The backdrop covers the network rail"
    );
    click(&ui, 37., 214.);
    key(&ui, "uplink://coop.example/rooms/coop");
    key(&ui, Key::Return);
    assert_eq!(
        submitted.borrow().as_str(),
        "uplink://coop.example/rooms/coop"
    );
    key(&ui, Key::Escape);
    assert!(!ui.get_adding_network());
}

#[test]
fn avatar_picker_opens_only_on_instances_that_support_it() {
    let (ui, surface) = open();
    let net = ui.global::<NetworkUi>();
    net.set_manages(true);
    net.set_panel("general".into());
    let actions = Rc::new(RefCell::new(Vec::new()));
    let capture = actions.clone();
    net.on_action(move |action, _| capture.borrow_mut().push(action.to_string()));
    platform::update_timers_and_animations();
    let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
    surface.draw_if_needed(|r| {
        r.render(&mut pixels, 388);
    });
    platform::update_timers_and_animations();
    let bounds = net.get_drop_bounds();
    assert!(bounds.w > 0.);
    click(&ui, bounds.x + 30., bounds.y + 30.);
    assert!(actions.borrow().is_empty());
    net.set_avatar_editable(true);
    click(&ui, bounds.x + 30., bounds.y + 30.);
    assert_eq!(*actions.borrow(), ["pick"]);
}

#[test]
fn russian_paste_in_invitation_input() {
    let (ui, _) = open();
    let submitted = Rc::new(RefCell::new(String::new()));
    let result = submitted.clone();
    ui.on_join_room(move |url| *result.borrow_mut() = url.to_string());
    click(&ui, 68., 174.);
    assert!(ui.get_adding_network());
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    key(&ui, "м");
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    key(&ui, Key::Return);
    assert_eq!(submitted.borrow().as_str(), "uplink://join/after-hours");
}

#[test]
fn only_the_pencil_starts_editing_at_the_end_of_the_nickname() {
    let (ui, surface) = open();
    let weak = ui.as_weak();
    ui.on_rename(move |nick| {
        let ui = weak.unwrap();
        ui.set_nickname(nick);
        ui.set_editing(false);
    });
    let pencil_pixels = || {
        let mut buffer = vec![slint::Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut buffer, 388);
        });
        (68..85)
            .flat_map(|y| (93..115).map(move |x| y * 388 + x))
            .filter(|&index| buffer[index].r > 100)
            .count()
    };
    let idle = pencil_pixels();
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(56., 77.),
    });
    assert!(
        pencil_pixels() > idle + 5,
        "Наведение должно показать карандаш"
    );
    click(&ui, 56., 77.);
    assert!(!ui.get_editing());
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(103., 77.),
    });
    click(&ui, 103., 77.);
    assert!(ui.get_editing());
    key(&ui, "Черновик");
    key(&ui, Key::Escape);
    assert!(!ui.get_editing());
    assert_eq!(ui.get_nickname(), "Ranazy");

    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(56., 77.),
    });
    click(&ui, 56., 77.);
    assert!(!ui.get_editing());
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(103., 77.),
    });
    click(&ui, 103., 77.);
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    key(&ui, "М");
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    key(&ui, Key::Return);
    assert_eq!(ui.get_nickname(), "Ranazyuplink://join/after-hours");
    assert!(!ui.get_editing());
}

#[test]
fn clicking_outside_commits_once_and_preserves_the_clicked_control() {
    let (ui, _) = open();
    let saved = Rc::new(RefCell::new(Vec::new()));
    let saves = saved.clone();
    let weak = ui.as_weak();
    ui.on_rename(move |name| {
        saves.borrow_mut().push(name.to_string());
        let ui = weak.unwrap();
        ui.set_nickname(name);
        ui.set_editing(false);
    });
    let joined = Rc::new(RefCell::new(Vec::new()));
    let joins = joined.clone();
    ui.on_join_room(move |link| joins.borrow_mut().push(link.to_string()));

    click(&ui, 103., 77.);
    key(&ui, "1");
    click(&ui, 360., 402.);
    assert_eq!(*saved.borrow(), ["Ranazy1"]);
    assert!(!ui.get_editing());
    key(&ui, "X");
    assert_eq!(ui.get_nickname(), "Ranazy1");

    click(&ui, 108., 77.);
    key(&ui, "2");
    click(&ui, 68., 174.);
    assert!(!ui.get_editing());
    assert!(ui.get_adding_network());
    key(&ui, "abc");
    key(&ui, Key::Return);
    assert_eq!(*joined.borrow(), ["abc"]);
    key(&ui, Key::Escape);

    click(&ui, 115., 77.);
    key(&ui, "3");
    click(&ui, 68., 174.);
    assert!(ui.get_adding_network());
    key(&ui, "second");
    click(&ui, 335., 291.);
    assert!(!ui.get_editing());
    assert_eq!(*saved.borrow(), ["Ranazy1", "Ranazy12", "Ranazy123"]);
    assert_eq!(*joined.borrow(), ["abc", "second"]);
}

#[test]
fn clicking_room_content_or_title_commits_the_nickname() {
    let (ui, _) = open();
    ui.set_joined(true);
    ui.set_members(slint::ModelRc::new(slint::VecModel::from(vec![
        Member {
            nick: "Ranazy".into(),
            own: true,
            ..Default::default()
        },
        Member {
            nick: "Mira".into(),
            creator: true,
            ..Default::default()
        },
    ])));
    let weak = ui.as_weak();
    ui.on_rename(move |name| {
        let ui = weak.unwrap();
        ui.set_nickname(name);
        ui.set_editing(false);
    });
    for (x, y) in [(55., 290.), (68., 29.), (40., 109.)] {
        ui.set_nickname("Ranazy".into());
        click(&ui, 103., 77.);
        assert!(ui.get_editing());
        key(&ui, "1");
        click(&ui, x, y);
        assert_eq!(ui.get_nickname(), "Ranazy1", "click at {x}, {y}");
        assert!(!ui.get_editing());
    }
}

#[test]
fn only_creator_can_edit_room_title_and_click_away_to_save() {
    let (ui, _) = open();
    ui.set_joined(true);
    ui.set_room_name("LAN".into());
    let net = ui.global::<NetworkUi>();
    net.set_name("LAN".into());
    let weak = ui.as_weak();
    net.on_action(move |action, value| {
        assert_eq!(action, "name");
        let ui = weak.unwrap();
        ui.global::<NetworkUi>().set_name(value.clone());
        ui.set_room_name(value);
        ui.set_editing_room(false);
    });
    click(&ui, 305., 132.);
    assert_eq!(net.get_panel(), "");
    ui.set_is_creator(true);
    click(&ui, 305., 132.);
    assert_eq!(net.get_panel(), "general");
    platform::update_timers_and_animations();
    let bounds = net.get_drop_bounds();
    assert_eq!(
        (bounds.h, bounds.w, bounds.x, bounds.y),
        (84., 64., 36., 169.)
    );
    click(&ui, 164., 200.);
    assert!(ui.get_editing_room());
    key(&ui, "2");
    key(&ui, Key::Escape);
    assert_eq!(ui.get_room_name(), "LAN");
    click(&ui, 164., 200.);
    key(&ui, "3");
    click(&ui, 350., 465.);
    assert_eq!(ui.get_room_name(), "LAN3");
    assert!(!ui.get_editing_room());
}

#[test]
fn transferring_from_more_requires_confirmation_and_targets_the_selected_member() {
    let (ui, _) = open();
    let net = ui.global::<NetworkUi>();
    net.set_owner(true);
    net.set_manages(true);
    net.set_name("After hours".into());
    net.set_panel("people".into());
    net.set_people(slint::ModelRc::new(slint::VecModel::from(vec![
        Person {
            name: "Ranazy".into(),
            own: true,
            owner: true,
            ..Default::default()
        },
        Person {
            name: "Mira".into(),
            ip: "192.0.2.10".into(),
            ..Default::default()
        },
    ])));
    let called = Rc::new(RefCell::new(Vec::new()));
    let record = called.clone();
    net.on_action(move |k, v| record.borrow_mut().push((k.to_string(), v.to_string())));
    click(&ui, 342., 213.);
    click(&ui, 270., 247.);
    assert_eq!(net.get_modal(), "transfer");
    assert_eq!(net.get_target(), "192.0.2.10");
    assert!(called.borrow().is_empty());
}

#[test]
fn saved_network_password_can_be_revealed_again_without_resubmitting_it() {
    let (ui, surface) = open();
    slint::select_bundled_translation("ru").unwrap();
    let net = ui.global::<NetworkUi>();
    net.set_live(true);
    net.set_manages(true);
    net.set_password_on(true);
    let changes = Rc::new(RefCell::new(Vec::new()));
    let saved = changes.clone();
    let weak = ui.as_weak();
    net.on_action(move |action, value| {
        assert_eq!(action, "password");
        saved.borrow_mut().push(value.to_string());
        let ui = weak.unwrap();
        let net = ui.global::<NetworkUi>();
        net.set_password(value);
        net.set_password_revision(net.get_password_revision() + 1);
    });
    let saved = changes.clone();
    let weak = ui.as_weak();
    net.on_load_password(move || {
        weak.unwrap()
            .global::<NetworkUi>()
            .set_password(saved.borrow().last().cloned().unwrap_or_default().into());
    });
    net.set_panel("methods".into());
    click(&ui, 110., 309.);
    key(&ui, "new-password");
    key(&ui, Key::Return);
    assert_eq!(changes.borrow().as_slice(), ["new-password"]);
    key(&ui, Key::Return);
    assert_eq!(changes.borrow().len(), 1);
    let text_pixels = || {
        let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut pixels, 388);
        });
        (297..319)
            .flat_map(|y| pixels[y * 388 + 51..y * 388 + 123].iter().copied())
            .collect::<Vec<_>>()
    };
    let masked = text_pixels();
    click(&ui, 248., 308.);
    let revealed = text_pixels();
    assert!(masked != revealed, "The eye should show the saved text");
    click(&ui, 81., 127.);
    click(&ui, 188., 127.);
    assert_eq!(net.get_panel(), "methods");
    assert!(text_pixels() == masked, "Reopened field should be masked");
    click(&ui, 248., 308.);
    assert!(
        text_pixels() == revealed,
        "Reopened field should reveal the saved value"
    );
}

#[test]
fn credentials_submit_server_name_and_secret_from_the_native_form() {
    let (ui, surface) = open();
    let net = ui.global::<NetworkUi>();
    click(&ui, 68., 174.);
    assert!(ui.get_adding_network());
    click(&ui, 274., 253.);
    let args = Rc::new(RefCell::new(Vec::new()));
    let sent = args.clone();
    net.on_login(move |server, name, password| {
        *sent.borrow_mut() = vec![server.to_string(), name.to_string(), password.to_string()];
    });
    assert!(net.get_credentials());
    key(&ui, "after-hours.example");
    key(&ui, Key::Return);
    key(&ui, "После работы");
    key(&ui, Key::Return);
    key(&ui, "ui-fixture");
    key(&ui, Key::Return);
    assert_eq!(
        args.borrow().as_slice(),
        ["after-hours.example", "После работы", "ui-fixture"]
    );
    net.set_joining(true);
    platform::update_timers_and_animations();
    click(&ui, 5., 5.);
    assert!(ui.get_adding_network());
    net.set_join_step(2);
    net.set_join_failure("Windows denied access to the ZeroTier root file (planet).".into());
    net.set_joining(false);
    platform::update_timers_and_animations();
    let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
    surface.request_redraw();
    surface.draw_if_needed(|r| { r.render(&mut pixels, 388); });
    let red = |p: &slint::Rgb8Pixel| p.r > 170 && p.r as u16 > p.g as u16 + 45;
    assert!(pixels.chunks(388).any(|row| row[34..58].iter().any(red)), "Failed step has a red marker");
    args.borrow_mut().clear();
    click(&ui, 304., 370.);
    assert_eq!(args.borrow().as_slice(), ["after-hours.example", "После работы", "ui-fixture"]);
    args.borrow_mut().clear();
    click(&ui, 223., 370.);
    assert!(net.get_join_failure().is_empty());
    assert!(ui.get_adding_network());
    key(&ui, Key::Return);
    key(&ui, Key::Return);
    key(&ui, Key::Return);
    assert_eq!(args.borrow().as_slice(), ["after-hours.example", "После работы", "ui-fixture"]);
}

#[test]
fn version_opens_release_notes_without_starting_download() {
    let (ui, _surface) = open();
    let update = ui.global::<UpdateUi>();
    update.set_version("0.1.0".into());
    update.set_target("0.1.1".into());
    update.set_phase(UpdatePhase::Found);
    click(&ui, 120., 29.);
    assert!(update.get_dialog_open());
    assert_eq!(update.get_phase(), UpdatePhase::Found);
    key(&ui, Key::Escape);
    assert!(!update.get_dialog_open());
}

#[test]
fn vps_wizard_validates_fields_and_submits_the_confirmed_request() {
    let (ui, surface) = open();
    let wizard = ui.global::<VpsUi>();
    wizard.on_check(|field, value| uplink::vps::field_error(&field, &value).into());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    wizard.on_install(move |name,ip,port,user,password,key,fp| {
        seen.borrow_mut().push([name,ip,port,user,password,key,fp].map(|s|s.to_string()));
    });
    let primary = || {
        platform::update_timers_and_animations();
        let mut pixels = vec![slint::Rgb8Pixel::default();388*540];
        surface.request_redraw();
        surface.draw_if_needed(|r| { r.render(&mut pixels,388); });
        let rows: Vec<_> = pixels.chunks(388).enumerate().filter(|(_,row)| {
            row.iter().filter(|p|p.g>185 && p.r>130 && p.b<150).count()>125
        }).map(|(y,_)|y).collect();
        assert!(!rows.is_empty());
        click(&ui,194.,rows[rows.len()/2] as f32);
    };
    ui.set_creating_network(true);
    primary();
    key(&ui,"Co-op");
    key(&ui,Key::Return);
    primary();
    key(&ui,"192.0.2.10");
    key(&ui,Key::Return);
    key(&ui,"root");
    key(&ui,Key::Return);
    key(&ui,"fixture");
    key(&ui,Key::Return);
    primary();
    let fingerprint="SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    key(&ui,fingerprint);
    key(&ui,Key::Return);
    primary();
    assert_eq!(calls.borrow().len(),1);
    assert_eq!(calls.borrow()[0], ["Co-op","192.0.2.10","22","root","fixture","",fingerprint]);
    wizard.set_step(2);
    wizard.set_failure("Fixture failure".into());
    click(&ui,4.,4.);
    assert!(!ui.get_creating_network());
}
