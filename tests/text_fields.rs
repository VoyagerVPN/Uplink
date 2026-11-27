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

#[test]
fn the_entire_title_bar_drags_except_for_active_controls() {
    use i_slint_core::{InternalToken, window::WindowAdapterInternal};
    use slint::Window;
    use slint::platform::{Renderer, software_renderer::SoftwareRenderer};
    use std::cell::Cell;

    struct TitleBarWindow {
        window: Window,
        renderer: SoftwareRenderer,
        moves: Cell<u32>,
    }
    impl WindowAdapter for TitleBarWindow {
        fn window(&self) -> &Window { &self.window }
        fn renderer(&self) -> &dyn Renderer { &self.renderer }
        fn size(&self) -> PhysicalSize { PhysicalSize::new(388, 540) }
        fn internal(&self, _: InternalToken) -> Option<&dyn WindowAdapterInternal> { Some(self) }
    }
    impl WindowAdapterInternal for TitleBarWindow {
        fn start_window_move(&self) { self.moves.set(self.moves.get() + 1); }
    }
    struct TitleBarPlatform(Rc<TitleBarWindow>);
    impl Platform for TitleBarPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }

    let surface = Rc::new_cyclic(|weak| TitleBarWindow {
        window: Window::new(weak.clone()),
        renderer: SoftwareRenderer::new_with_repaint_buffer_type(RepaintBufferType::NewBuffer),
        moves: Cell::new(0),
    });
    platform::set_platform(Box::new(TitleBarPlatform(surface.clone()))).unwrap();
    let ui = RoomWindow::new().unwrap();
    let updates = ui.global::<UpdateUi>();
    updates.set_version("0.3.2".into());
    ui.show().unwrap();
    ui.window().dispatch_event(WindowEvent::Resized { size: slint::LogicalSize::new(388., 540.) });

    let drag = |x, y| {
        let position = LogicalPosition::new(x, y);
        let end = LogicalPosition::new(x, y + 10.);
        let button = PointerEventButton::Left;
        ui.window().dispatch_event(WindowEvent::PointerMoved { position });
        ui.window().dispatch_event(WindowEvent::PointerPressed { position, button });
        ui.window().dispatch_event(WindowEvent::PointerMoved { position: end });
        ui.window().dispatch_event(WindowEvent::PointerReleased { position: end, button });
        surface.moves.replace(0)
    };

    for (x, y) in [(3., 3.), (48., 25.), (154., 25.), (182., 25.), (235., 25.), (385., 25.)] {
        assert_eq!(drag(x, y), 1, "Blank title-bar point {x},{y} should start a move");
    }
    assert_eq!(drag(48., 62.), 0, "The content should stay outside the drag area");
    assert_eq!(drag(118., 25.), 0, "The version is a button while idle");
    updates.set_phase(UpdatePhase::Checking);
    assert_eq!(drag(118., 25.), 1, "An inactive version should allow dragging");
    assert_eq!(drag(285., 25.), 0, "A settings-button drag must not move the window");
    ui.set_settings_open(false);
    click(&ui, 285., 25.);
    assert!(ui.get_settings_open(), "The settings button still responds to a click");
}

fn key(ui: &RoomWindow, text: impl Into<slint::SharedString>) {
    let text = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn count_color(surface: &MinimalSoftwareWindow, color: (u8, u8, u8)) -> usize {
    let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
    surface.request_redraw();
    surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, 388); });
    pixels.iter().filter(|p| (p.r, p.g, p.b) == color).count()
}

fn capture_settings_qa(surface: &MinimalSoftwareWindow, name: &str) {
    let Some(directory) = std::env::var_os("UPLINK_SETTINGS_QA_OUTPUT") else { return; };
    let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
    surface.request_redraw();
    surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, 388); });
    let bytes: Vec<_> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    image::save_buffer(std::path::Path::new(&directory).join(name), &bytes, 388, 540, image::ColorType::Rgb8).unwrap();
}

#[test]
fn avatar_save_stays_reachable_with_a_long_error_at_high_dpi() {
    let (ui, surface) = open();
    let network = ui.global::<NetworkUi>();
    network.set_modal("crop".into());
    let saves = Rc::new(RefCell::new(0));
    let received = saves.clone();
    network.on_action(move |action, _| {
        assert_eq!(action, "save-avatar");
        *received.borrow_mut() += 1;
    });
    for (language, scale) in [("ru", 1.25), ("en", 1.5)] {
        slint::select_bundled_translation(language).unwrap();
        let message = uplink::i18n::error_message(language, "The server upload limit is too low for this avatar.");
        network.set_error(format!("{message} ").repeat(16).into());
        ui.window().dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
        let width = (388. * scale) as usize;
        let height = (540. * scale) as usize;
        surface.set_size(PhysicalSize::new(width as u32, height as u32));
        let mut pixels = vec![slint::Rgb8Pixel::default(); width * height];
        surface.request_redraw();
        surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, width); });
        if let Some(output) = std::env::var_os("UPLINK_DIALOG_DPI_CAPTURE") {
            let output = std::path::PathBuf::from(output).join(format!("avatar-{language}.png"));
            let bytes: Vec<_> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
            image::save_buffer(output, &bytes, width as u32, height as u32, image::ColorType::Rgb8).unwrap();
        }
        let is_lime = |pixel: &slint::Rgb8Pixel| matches!((pixel.r, pixel.g, pixel.b), (191, 236, 112) | (206, 246, 133));
        let save_row = pixels.chunks(width).enumerate().rev().find(|(_, row)| {
            row.iter().skip((240. * scale) as usize).filter(|p| is_lime(p)).count() >= (30. * scale) as usize
        }).map(|(y, _)| y).expect("Save button stays inside the window");
        click(&ui, 286., save_row as f32 / scale - 5.);
    }
    assert_eq!(*saves.borrow(), 2);
}

#[test]
fn open_language_list_allows_the_first_click_on_settings_and_switches() {
    let (ui, surface) = open();
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
    ui.set_language_code("ru".into());
    ui.set_language_name("Русский".into());
    capture_settings_qa(&surface, "settings.png");
    click(&ui, 332., 236.);
    assert_eq!(*events.borrow(), ["autostart"], "The switch is at its usual position");
    click(&ui, 292., 172.);
    capture_settings_qa(&surface, "language-popup.png");
    click(&ui, 332., 236.);
    assert_eq!(*events.borrow(), ["autostart", "autostart"], "Opening the language list keeps the switch in place and clickable");
    let selection = Rc::new(RefCell::new(String::new()));
    let selected = selection.clone();
    ui.on_change_language(move |code| *selected.borrow_mut() = code.to_string());
    click(&ui, 292., 172.);
    click(&ui, 250., 208.);
    assert_eq!(selection.borrow().as_str(), "en", "The first language is selectable below the chevron");
    click(&ui, 292., 172.);
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
    click(&ui, 37., 120.);
    assert_eq!(*selections.borrow(), ["uplink://coop.example/rooms/coop"]);
    let submitted = Rc::new(RefCell::new(String::new()));
    let result = submitted.clone();
    ui.on_join_room(move |url| *result.borrow_mut() = url.to_string());
    click(&ui, 37., 159.);
    assert!(ui.get_adding_network());
    click(&ui, 37., 120.);
    assert!(
        !ui.get_adding_network(),
        "Clicking the backdrop dismisses the join form"
    );
    assert_eq!(
        selections.borrow().len(),
        1,
        "The backdrop covers the network rail"
    );
    click(&ui, 37., 159.);
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
    click(&ui, 68., 116.);
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
fn settings_nickname_starts_editing_at_the_end_and_escape_discards_changes() {
    let (ui, _) = open();
    ui.set_settings_open(true);
    let weak = ui.as_weak();
    ui.on_rename(move |nick| {
        let ui = weak.unwrap();
        ui.set_nickname(nick);
        ui.set_editing(false);
    });
    click(&ui, 80., 124.);
    assert!(!ui.get_editing());
    click(&ui, 330., 124.);
    assert!(ui.get_editing());
    key(&ui, "Черновик");
    key(&ui, Key::Escape);
    assert!(!ui.get_editing());
    assert_eq!(ui.get_nickname(), "Ranazy");

    click(&ui, 252., 124.);
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: Key::Control.into() });
    key(&ui, "М");
    ui.window().dispatch_event(WindowEvent::KeyReleased { text: Key::Control.into() });
    key(&ui, Key::Return);
    assert_eq!(ui.get_nickname(), "Ranazyuplink://join/after-hours");
    assert!(!ui.get_editing());
}

#[test]
fn leaving_settings_saves_the_nickname_once_and_preserves_the_next_action() {
    let (ui, _) = open();
    ui.set_settings_open(true);
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

    click(&ui, 330., 124.);
    key(&ui, "1");
    click(&ui, 35., 77.);
    assert_eq!(*saved.borrow(), ["Ranazy1"]);
    assert!(!ui.get_editing());
    assert!(!ui.get_settings_open());
    click(&ui, 68., 116.);
    assert!(ui.get_adding_network());
    key(&ui, "abc");
    key(&ui, Key::Return);
    assert_eq!(*joined.borrow(), ["abc"]);
}

#[test]
fn a_network_manager_can_edit_the_title_and_click_away_to_save() {
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
    click(&ui, 355., 77.);
    click(&ui, 10., 430.);
    assert_eq!(net.get_panel(), "");
    net.set_manages(true);
    net.set_owner(true);
    click(&ui, 355., 77.);
    click(&ui, 210., 147.);
    assert_eq!(net.get_panel(), "general");
    platform::update_timers_and_animations();
    let bounds = net.get_drop_bounds();
    assert_eq!(
        (bounds.h, bounds.w, bounds.x, bounds.y),
        (84., 64., 32., 165.)
    );
    let edit_title = || click(&ui, bounds.x + 128., bounds.y + 31.);
    edit_title();
    assert!(ui.get_editing_room());
    key(&ui, "2");
    key(&ui, Key::Escape);
    assert_eq!(ui.get_room_name(), "LAN");
    edit_title();
    key(&ui, "3");
    click(&ui, 350., 465.);
    assert_eq!(ui.get_room_name(), "LAN3");
    assert!(!ui.get_editing_room());
}

#[test]
fn unsupported_member_actions_do_not_start_ssh_or_open_the_nickname_editor() {
    let (ui, surface) = open();
    ui.set_joined(true);
    ui.set_room_name("Legacy network".into());
    let network = ui.global::<NetworkUi>();
    network.set_live(true);
    network.set_manages(true);
    network.set_owner(true);
    ui.set_members(slint::ModelRc::new(slint::VecModel::from(vec![Member {
        nick: "Mira".into(),
        ip: "192.0.2.10".into(),
        ..Default::default()
    }])));
    let requests = Rc::new(RefCell::new(Vec::new()));
    let received = requests.clone();
    ui.on_ssh_member(move |action, _| received.borrow_mut().push(action));

    click(&ui, 355., 148.);
    capture_settings_qa(&surface, "legacy-member-menu.png");
    click(&ui, 252., 183.);
    assert!(requests.borrow().is_empty(), "Legacy servers cannot receive SSH requests");
    let background = || {
        let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, 388); });
        pixels[450 * 388 + 10]
    };
    let before = background();
    click(&ui, 252., 212.);
    assert_eq!(background(), before, "The disabled nickname action keeps the window outside the popup unchanged");
    click(&ui, 252., 241.);
    assert_eq!(network.get_modal(), "remove", "Supported member management stays available");
}

#[test]
fn a_supported_server_can_receive_ssh_requests_from_the_member_menu() {
    let (ui, surface) = open();
    ui.set_joined(true);
    ui.set_room_name("Updated network".into());
    let network = ui.global::<NetworkUi>();
    network.set_live(true);
    network.set_ssh_request_supported(true);
    ui.set_members(slint::ModelRc::new(slint::VecModel::from(vec![Member {
        nick: "Mira".into(),
        ip: "192.0.2.10".into(),
        ..Default::default()
    }])));
    let requests = Rc::new(RefCell::new(Vec::new()));
    let received = requests.clone();
    ui.on_ssh_member(move |action, _| received.borrow_mut().push(action));
    click(&ui, 355., 148.);
    ui.window().dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(350., 179.) });
    capture_settings_qa(&surface, "supported-member-menu.png");
    click(&ui, 350., 179.);
    assert_eq!(requests.borrow().as_slice(), ["request"]);
}

#[test]
fn transferring_from_more_requires_confirmation_and_targets_the_selected_member() {
    let (ui, surface) = open();
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
    assert_eq!(count_color(&surface, (140, 146, 154)), 0, "Mouse opening leaves Cancel without a focus outline");
    key(&ui, Key::Return);
    assert!(net.get_modal().is_empty(), "Enter activates Cancel by default");
    assert!(called.borrow().is_empty());
    key(&ui, Key::Return);
    click(&ui, 270., 247.);
    assert_eq!(net.get_modal(), "transfer", "Enter opens the participant menu again");
    key(&ui, Key::Tab);
    assert!(count_color(&surface, (140, 146, 154)) > 0, "Tab shows the keyboard focus");
    key(&ui, Key::Escape);
    assert!(net.get_modal().is_empty());
    assert!(called.borrow().is_empty());
}

#[test]
fn join_dialog_shows_a_secondary_button_for_vps_setup() {
    let (ui, surface) = open();
    ui.set_adding_network(true);
    assert!(count_color(&surface, (37, 39, 43)) > 100, "The VPS entry has the secondary button fill");
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
    click(&ui, 68., 116.);
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
    net.set_join_failure("Invalid Uplink network profile".into());
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
    key(&ui, Key::Return);
    assert!(update.get_dialog_open(), "Enter reopens release notes from the restored version button");
}

#[test]
fn update_warning_explains_the_failure_and_waits_for_explicit_retry() {
    let (ui, surface) = open();
    slint::select_bundled_translation("ru").unwrap();
    let update = ui.global::<UpdateUi>();
    update.set_version("0.3.0".into());
    update.set_target("0.3.1".into());
    update.set_phase(UpdatePhase::InstallFailed);
    update.set_failure(UpdateFailure::Permission);
    let checks = Rc::new(RefCell::new(0));
    let received = checks.clone();
    let weak = ui.as_weak();
    update.on_check(move || {
        *received.borrow_mut() += 1;
        weak.upgrade().unwrap().global::<UpdateUi>().set_phase(UpdatePhase::Checking);
    });
    click(&ui, 148., 29.);
    assert!(update.get_dialog_open());
    assert_eq!(*checks.borrow(), 0);
    assert!(update.get_failure_detail().contains("подтвердите запрос Windows"));

    let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
    surface.request_redraw();
    surface.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, 388);
    });
    if let Some(output) = std::env::var_os("UPLINK_UPDATE_ERROR_CAPTURE") {
        let bytes: Vec<_> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::save_buffer(output, &bytes, 388, 540, image::ColorType::Rgb8).unwrap();
    }
    let row = pixels.chunks(388).position(|row| {
        row.iter().filter(|p| (p.r, p.g, p.b) == (191, 236, 112)).count() > 35
    }).unwrap();
    click(&ui, 315., row as f32 + 8.);
    assert_eq!(*checks.borrow(), 1);
    assert!(update.get_dialog_open());
    assert_eq!(update.get_phase(), UpdatePhase::Checking);

    update.set_phase(UpdatePhase::Offline);
    update.set_failure(UpdateFailure::Download);
    assert!(update.get_failure_detail().contains("Загрузка прервалась"));
    platform::update_timers_and_animations();
    key(&ui, Key::Escape);
    assert!(!update.get_dialog_open());
}

#[test]
fn closing_join_dialog_restores_the_join_button() {
    let (ui, _) = open();
    click(&ui, 100., 116.);
    assert!(ui.get_adding_network());
    key(&ui, Key::Escape);
    assert!(!ui.get_adding_network());
    key(&ui, Key::Return);
    assert!(ui.get_adding_network(), "Enter reopens the join dialog from its button");
}

#[test]
fn release_notes_wait_until_the_join_dialog_closes() {
    let (ui, _) = open();
    let update = ui.global::<UpdateUi>();
    update.set_phase(UpdatePhase::Ready);
    ui.set_adding_network(true);
    update.set_dialog_open(true);

    key(&ui, Key::Escape);
    assert!(!ui.get_adding_network());
    assert!(update.get_dialog_open());
    key(&ui, Key::Escape);
    assert!(!update.get_dialog_open());
}

#[test]
fn vps_wizard_validates_fields_and_submits_the_confirmed_request() {
    create_from_wizard(false);
}

#[test]
fn vps_wizard_forwards_provider_proxy_settings() {
    create_from_wizard(true);
}

#[test]
fn a_network_owner_can_confirm_a_server_update_from_the_network_menu() {
    run_server_upgrade_from_network_menu(true);
}

#[test]
fn a_legacy_server_only_asks_for_ssh_access_when_the_owner_sets_it_up() {
    run_server_upgrade_from_network_menu(false);
}

fn run_server_upgrade_from_network_menu(saved_access: bool) {
    let (ui, surface) = open();
    slint::select_bundled_translation("en").unwrap();
    ui.set_joined(true);
    ui.set_room_name("After hours".into());
    ui.set_room_server("lan.example".into());
    ui.set_invite_url("fixture".into());
    ui.global::<UpdateUi>().set_version("0.3.0".into());
    ui.set_rooms(slint::ModelRc::new(slint::VecModel::from(
        [("After hours", "AH", "fixture"), ("Co-op", "CO", "coop"), ("Arena", "A", "arena")]
            .into_iter().map(|(name, initials, url)| SavedRoom {
                name: name.into(), initials: initials.into(), url: url.into(), ..Default::default()
            }).collect::<Vec<_>>()
    )));
    ui.set_members(slint::ModelRc::new(slint::VecModel::from(
        [("Ranazy", 2, 28), ("Mira", 3, 37), ("Orbit", 4, 62), ("Bishop", 5, 0)]
            .into_iter().map(|(nick, id, ms)| Member {
                nick: nick.into(), ip: format!("10.42.7.{id}").into(), own: id == 2, creator: id == 2,
                ping: PingReading { bars: uplink::peers::signal_bars((ms > 0).then_some(ms)), ms: ms as i32 },
                ..Default::default()
            }).collect::<Vec<_>>()
    )));
    let network = ui.global::<NetworkUi>();
    network.set_live(true);
    network.set_manages(true);
    network.set_owner(true);
    network.set_server_revision(2);
    network.set_bundled_server_revision(3);
    let wizard = ui.global::<VpsUi>();
    wizard.on_check(|field, value| uplink::vps::field_error(&field, &value).into());
    wizard.on_has_upgrade_access(move |server| {
        assert_eq!(server, "lan.example");
        saved_access
    });
    let calls = Rc::new(RefCell::new(Vec::new()));
    let received = calls.clone();
    wizard.on_upgrade(move |server| {
        received.borrow_mut().push([server.to_string(), String::new(), String::new(), String::new(), String::new(), String::new()]);
    });
    let received = calls.clone();
    wizard.on_upgrade_with_access(move |server, ip, port, user, password, identity| {
        received.borrow_mut().push([server, ip, port, user, password, identity].map(|s| s.to_string()));
    });
    wizard.on_install(|_, _, _, _, _, _, _, _, _| panic!("The update must not create a network"));

    let capture = |name| {
        if let Some(output) = std::env::var_os("UPLINK_UPGRADE_CAPTURE") {
            let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
            surface.request_redraw();
            surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, 388); });
            let height = if name == "network.png" { 274 } else { 540 };
            let bytes: Vec<_> = pixels.into_iter().take(388 * height as usize).flat_map(|p| [p.r, p.g, p.b]).collect();
            let path = std::path::Path::new(&output).join(name);
            image::save_buffer(path, &bytes, 388, height, image::ColorType::Rgb8).unwrap();
        }
    };
    if saved_access { capture("network.png"); }
    click(&ui, 355., 74.);
    if saved_access { capture("network-menu.png"); }
    ui.window().dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(172., 175.) });
    click(&ui, 172., 175.);
    assert!(ui.get_creating_network(), "Update server opens the VPS dialog");
    capture(if saved_access { "confirm.png" } else { "legacy-confirm.png" });
    let primary_y = || {
        let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, 388); });
        let rows = pixels.chunks(388).enumerate().filter(|(_, row)| {
            row.iter().filter(|p| p.g > 185 && p.r > 130 && p.b < 150).count() > 55
        }).map(|(y, _)| y).collect::<Vec<_>>();
        rows[rows.len() / 2] as f32
    };
    let primary = || click(&ui, 194., primary_y());
    assert!(calls.borrow().is_empty(), "The user confirms downtime before the update starts");
    primary();
    if saved_access {
        assert_eq!(calls.borrow().as_slice(), [["lan.example", "", "", "", "", ""]]);
    } else {
        assert!(calls.borrow().is_empty());
        capture("legacy-access.png");
        let before_validation = primary_y();
        primary();
        capture("legacy-validation.png");
        assert_eq!(primary_y(), before_validation, "Validation must keep the form in place");
        key(&ui, "192.0.2.10");
        key(&ui, Key::Return);
        key(&ui, "root");
        key(&ui, Key::Return);
        key(&ui, "fixture");
        key(&ui, Key::Return);
        assert_eq!(calls.borrow().as_slice(), [["lan.example", "192.0.2.10", "22", "root", "fixture", ""]]);
    }
    key(&ui, Key::Escape);
    assert!(ui.get_creating_network(), "The update remains visible while the VPS works");
    if saved_access {
        wizard.set_step(4);
        let status_area = || {
            let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
            surface.request_redraw();
            surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, 388); });
            pixels[98 * 388..123 * 388].to_vec()
        };
        let updating = status_area();
        ui.set_room_availability("sync_failed".into());
        assert!(updating == status_area(), "A transient proxy error should keep the update status in the network view");
        slint::select_bundled_translation("ru").unwrap();
        capture("server-updating.png");
        wizard.set_waiting_for_network(true);
        wizard.set_upgrade_result("updated".into());
        capture("server-updated.png");
        wizard.invoke_finish_upgrade_status();
        slint::select_bundled_translation("en").unwrap();
    }
    wizard.set_upgrade_result("restored".into());
    wizard.set_failure("Fixture failure".into());
    platform::update_timers_and_animations();
    if saved_access { capture("restored.png"); }
    key(&ui, Key::Escape);
    assert!(!ui.get_creating_network(), "A completed result can be dismissed");
}

#[test]
fn a_server_update_notification_opens_the_vps_dialog_for_that_server() {
    let (ui, surface) = open();
    ui.set_room_server("selected.example".into());
    ui.global::<UpdateUi>().set_version("0.3.0".into());
    ui.global::<NoticesUi>().set_server_updates(slint::ModelRc::from([ServerUpdateNotice {
        server: "another-long-vps-hostname-for-the-fixture.example".into(),
        network: "fixture-network".into(),
        name: "Вечерняя сеть с очень длинным названием для друзей".into(),
        revision: 2,
    }]));
    let selected = Rc::new(RefCell::new(String::new()));
    let destination = selected.clone();
    ui.on_select_room(move |network| *destination.borrow_mut() = network.to_string());
    click(&ui, 248., 29.);
    let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
    surface.request_redraw();
    surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, 388); });
    if let Some(output) = std::env::var_os("UPLINK_UPGRADE_CAPTURE") {
        let bytes: Vec<_> = pixels.into_iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::save_buffer(std::path::Path::new(&output).join("notification.png"), &bytes, 388, 540, image::ColorType::Rgb8).unwrap();
    }
    click(&ui, 168., 126.);
    assert!(ui.get_creating_network());
    assert_eq!(selected.borrow().as_str(), "fixture-network");
}

fn create_from_wizard(proxy: bool) {
    let (ui, surface) = open();
    let wizard = ui.global::<VpsUi>();
    wizard.on_check(|field, value| uplink::vps::field_error(&field, &value).into());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    wizard.on_install(move |name,ip,port,user,password,key,host,bind,udp| {
        seen.borrow_mut().push([name,ip,port,user,password,key,host,bind,udp].map(|s|s.to_string()));
    });
    let primary_y = || {
        platform::update_timers_and_animations();
        let mut pixels = vec![slint::Rgb8Pixel::default();388*540];
        surface.request_redraw();
        surface.draw_if_needed(|r| { r.render(&mut pixels,388); });
        let rows: Vec<_> = pixels.chunks(388).enumerate().filter(|(_,row)| {
            row.iter().filter(|p|p.g>185 && p.r>130 && p.b<150).count()>55
        }).map(|(y,_)|y).collect();
        assert!(!rows.is_empty());
        rows[rows.len()/2] as f32
    };
    let primary = || click(&ui, 194., primary_y());
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
    if proxy {
        click(&ui, 194., primary_y() - 40.);
        primary();
        key(&ui, "proxy.example.org");
        key(&ui, Key::Return);
        key(&ui, "192.168.88.6");
        key(&ui, Key::Return);
    } else {
        primary();
    }
    let endpoint = if proxy { ["proxy.example.org", "192.168.88.6", "9993"] } else { ["", "", ""] };
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0][..6], ["Co-op","192.0.2.10","22","root","fixture",""]);
    assert_eq!(calls[0][6..], endpoint);
    wizard.set_step(2);
    if !proxy {
        let marker = || {
            platform::update_timers_and_animations();
            let mut pixels = vec![slint::Rgb8Pixel::default(); 388 * 540];
            surface.request_redraw();
            surface.draw_if_needed(|r| { r.render(&mut pixels, 388); });
            pixels.chunks(388).flat_map(|row| row[37..55].iter().copied()).collect::<Vec<_>>()
        };
        let first = marker();
        std::thread::sleep(std::time::Duration::from_millis(220));
        assert!(first != marker(), "The active installation step should animate");
        let net = ui.global::<NetworkUi>();
        net.set_manages(true);
        net.set_avatar_editable(true);
        ui.set_invite_url("uplink://coop.example/networks/fixture".into());
        wizard.set_network_key(ui.get_invite_url());
        wizard.set_invitation("uplink://coop.example/invite/fixture".into());
        let weak = ui.as_weak();
        net.on_action(move |action, _| {
            let ui = weak.unwrap();
            let net = ui.global::<NetworkUi>();
            match action.as_str() {
                "pick" => net.set_modal("crop".into()),
                "save-avatar" => { net.set_has_avatar(true); net.invoke_close_modal(); }
                _ => panic!("Unexpected avatar action"),
            }
        });
        let avatar_y = primary_y() - 130.;
        click(&ui, 67., avatar_y);
        assert_eq!(net.get_modal(), "crop", "Owner can edit the avatar on the result screen");
        key(&ui, Key::Escape);
        assert!(net.get_modal().is_empty());
        assert!(ui.get_creating_network());
        key(&ui, Key::Return);
        assert_eq!(net.get_modal(), "crop", "The result screen restores focus to its avatar");
        click(&ui, 291., primary_y());
        assert!(net.get_has_avatar());
        assert!(net.get_modal().is_empty());
        let copied = Rc::new(RefCell::new(String::new()));
        let value = copied.clone();
        ui.on_copy(move |link| { *value.borrow_mut() = link.to_string(); true });
        primary();
        assert_eq!(*copied.borrow(), "uplink://coop.example/invite/fixture");
        return;
    }
    wizard.set_failure("Fixture failure".into());
    click(&ui,4.,4.);
    assert!(!ui.get_creating_network());
}
