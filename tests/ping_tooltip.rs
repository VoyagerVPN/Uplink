use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{self, Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
use std::{cell::Cell, rc::Rc, time::Duration};

slint::include_modules!();
#[path = "../src/update_notes.rs"]
mod update_notes;

struct TestDesktop {
    surface: Rc<MinimalSoftwareWindow>,
    time_ms: Rc<Cell<u64>>,
}

#[test]
fn redesign_reference_frames() {
    let Ok(out) = std::env::var("UPLINK_REDESIGN_OUTPUT") else { return };
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: Rc::new(Cell::new(10)),
    })).unwrap();
    let ui = RoomWindow::new().unwrap();
    slint::select_bundled_translation("ru").unwrap();
    ui.global::<UpdateUi>().set_version("0.3.0".into());
    ui.set_nickname("Ranazy".into());
    ui.set_joined(true);
    ui.set_invite_url("fixture".into());
    ui.set_room_name("После работы".into());
    ui.set_room_server("after-hours.example".into());
    ui.set_password_shareable(true);
    let net = ui.global::<NetworkUi>();
    net.set_manages(true);
    net.set_link_on(true);
    net.set_password_on(true);
    net.set_qr_on(true);
    ui.set_rooms(slint::ModelRc::new(slint::VecModel::from(
        [("После работы", "ПР", "fixture"), ("Кооп", "К", "coop"), ("Арена", "А", "arena"),
            ("Еловый бор", "Ел", "pine"), ("Ночная смена", "НС", "night")]
            .into_iter().map(|(name, initials, url)| SavedRoom {
                name: name.into(), initials: initials.into(), url: url.into(),
                server: format!("{url}.example").into(), ..Default::default()
            }).collect::<Vec<_>>()
    )));
    ui.set_members(slint::ModelRc::new(slint::VecModel::from(
        [("Bishop", 17, 0), ("Kaiser", 15, 0), ("Mira", 10, 24), ("Orbit", 12, 93), ("Ranazy", 11, 38),
            ("Базальт", 13, 186), ("Грай", 16, 0), ("Еловый", 14, 0)]
            .into_iter().map(|(nick, ip, ms)| Member {
                nick: nick.into(), ip: format!("192.0.2.{ip}").into(), own: ip == 11, creator: ip == 11,
                admin: ip == 12, ping: PingReading { bars: uplink::peers::signal_bars((ms > 0).then_some(ms)), ms: ms as i32 },
                ..Default::default()
            }).collect::<Vec<_>>()
    )));
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    let capture = |name| {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        platform::update_timers_and_animations();
        surface.request_redraw();
        assert!(surface.draw_if_needed(|r| { r.render(&mut pixels, 388); }));
        let bytes = pixels.into_iter().flat_map(|p| [p.r, p.g, p.b]).collect::<Vec<_>>();
        image::save_buffer(std::path::Path::new(&out).join(format!("{name}.png")), &bytes,
            388, 540, image::ColorType::Rgb8).unwrap();
    };
    let click = |x, y| {
        use slint::platform::PointerEventButton;
        let position = LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        ui.window().dispatch_event(WindowEvent::PointerMoved { position });
        ui.window().dispatch_event(WindowEvent::PointerPressed { position, button });
        ui.window().dispatch_event(WindowEvent::PointerReleased { position, button });
    };
    capture("rust-roster");
    click(355., 74.);
    capture("rust-network-menu");
    click(10., 480.);
    ui.global::<UpdateUi>().set_phase(UpdatePhase::Found);
    ui.global::<UpdateUi>().set_target("0.3.1".into());
    click(248., 31.);
    capture("rust-notifications");
    click(10., 480.);
    ui.set_settings_open(true);
    ui.set_language_name("Русский".into());
    capture("rust-settings");
    ui.set_settings_open(false);
    ui.set_welcome_open(true);
    capture("rust-welcome");
    ui.set_welcome_open(false);
    let ssh = ui.global::<SshUi>();
    ssh.set_requester("Orbit".into());
    ssh.set_network_name("После работы".into());
    ssh.set_initials("ПР".into());
    ssh.set_active_id("fixture".into());
    capture("rust-ssh-request");
    ssh.set_active_id("".into());
    ui.set_adding_network(true);
    capture("rust-join");
    net.set_credentials(true);
    capture("rust-join-password");
}
impl Platform for TestDesktop {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.surface.clone())
    }
    fn duration_since_start(&self) -> Duration {
        Duration::from_millis(self.time_ms.get())
    }
}

#[test]
fn hovering_server_signal_displays_and_dismisses_custom_tooltip() {
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let time_ms = Rc::new(Cell::new(10));
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: time_ms.clone(),
    }))
    .unwrap();
    let ui = RoomWindow::new().unwrap();
    ui.set_joined(true);
    ui.set_members(slint::ModelRc::new(slint::VecModel::from(vec![
        Member {
            own: true,
            ping: PingReading { bars: 3, ms: 38 },
            ..Default::default()
        },
        Member {
            nick: "Bishop".into(),
            ..Default::default()
        },
    ])));
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    let render = || {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut pixels, 388);
        });
        pixels
    };
    let before = render();
    // У второй строки нет замера. Все три полоски серые, место для числа свободно.
    for x in [173, 178, 183] {
        assert!(
            (159..187).any(|y| {
                let p = before[y * 388 + x];
                (p.r, p.g, p.b) == (82, 86, 92)
            }),
            "Нет серой полоски при x={x}"
        );
    }
    assert!((159..187).all(|y| (191..234).all(|x| before[y * 388 + x].r < 80)));

    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(180., 145.),
    });
    time_ms.set(1500);
    platform::update_timers_and_animations();
    let shown = render();
    // Фон всплывающей карточки занимает заметную область за пределами самого индикатора.
    let card_pixels = |pixels: &[Rgb8Pixel]| {
        pixels
            .iter()
            .filter(|p| (p.r, p.g, p.b) == (48, 51, 56))
            .count()
    };
    assert!(
        card_pixels(&shown) > card_pixels(&before) + 500,
        "Подсказка не появилась"
    );
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(5., 5.),
    });
    time_ms.set(3000);
    platform::update_timers_and_animations();
    assert_eq!(card_pixels(&render()), card_pixels(&before));
}

#[test]
fn invitation_copy_shows_a_check_for_two_seconds() {
    use slint::platform::PointerEventButton;
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let clock = Rc::new(Cell::new(10));
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: clock.clone(),
    }))
    .unwrap();
    let ui = RoomWindow::new().unwrap();
    let net = ui.global::<NetworkUi>();
    net.set_modal("invite".into());
    net.set_method("link".into());
    net.set_link_on(true);
    net.set_link("uplink://join/after-hours".into());
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    net.on_clipboard(move |text| {
        assert_eq!(text, "uplink://join/after-hours");
        count.set(count.get() + 1);
        true
    });
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    let check_visible = || {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut pixels, 388);
        });
        (290..326).any(|y| {
            (318..350).any(|x| {
                let p = pixels[y * 388 + x];
                p.g > 140 && p.g > p.b.saturating_add(45)
            })
        })
    };
    let position = LogicalPosition::new(334., 308.);
    let button = PointerEventButton::Left;
    assert!(!check_visible());
    ui.window()
        .dispatch_event(WindowEvent::PointerPressed { position, button });
    ui.window()
        .dispatch_event(WindowEvent::PointerReleased { position, button });
    platform::update_timers_and_animations();
    assert_eq!(calls.get(), 1);
    assert!(check_visible());
    clock.set(2011);
    platform::update_timers_and_animations();
    assert!(!check_visible());
}

#[test]
fn address_shows_copy_on_hover_then_acknowledges_each_click_for_two_seconds() {
    verify_address_copy(false);
}

#[test]
fn copying_the_server_address_inside_network_actions_shows_a_receipt() {
    verify_address_copy(true);
}

fn verify_address_copy(server: bool) {
    use slint::platform::PointerEventButton;
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let time = Rc::new(Cell::new(10));
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: time.clone(),
    }))
    .unwrap();
    let ui = RoomWindow::new().unwrap();
    ui.set_joined(true);
    ui.set_members(slint::ModelRc::new(slint::VecModel::from(vec![Member {
        own: true,
        ip: "192.0.2.11".into(),
        ..Default::default()
    }])));
    ui.set_room_server("lan".into());
    let calls = Rc::new(Cell::new(0));
    let copied = calls.clone();
    ui.on_copy(move |address| {
        assert_eq!(address, if server { "lan" } else { "192.0.2.11" });
        copied.set(copied.get() + 1);
        true
    });
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    let render = || {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut pixels, 388);
        });
        pixels
    };
    let click_at = |at| {
        let button = PointerEventButton::Left;
        ui.window().dispatch_event(WindowEvent::PointerMoved { position: at });
        ui.window().dispatch_event(WindowEvent::PointerPressed { position: at, button });
        ui.window().dispatch_event(WindowEvent::PointerReleased { position: at, button });
        platform::update_timers_and_animations();
    };
    if server {
        click_at(LogicalPosition::new(355., 77.));
        click_at(LogicalPosition::new(215., 110.));
        assert_eq!(calls.get(), 1);
        let receipt = |pixels: &[Rgb8Pixel]| {
            (102..120).any(|y| (122..145).any(|x| {
                let p = pixels[y * 388 + x];
                p.g > 140 && p.g > p.b.saturating_add(40)
            }))
        };
        assert!(receipt(&render()));
        time.set(2020);
        platform::update_timers_and_animations();
        assert!(!receipt(&render()));
        return;
    }
    let at = LogicalPosition::new(258., 145.);
    let hover = |pos| {
        ui.window()
            .dispatch_event(WindowEvent::PointerMoved { position: pos })
    };
    let click = || click_at(at);
    let advance = |t| {
        time.set(t);
        platform::update_timers_and_animations();
    };
    let icon_pixels = |pixels: &[Rgb8Pixel], green| {
        let (xs, ys) = (288..309, 135..157);
        ys.flat_map(|y| xs.clone().map(move |x| y * 388 + x))
            .filter(|&i| {
                let p = pixels[i];
                if green {
                    p.g > 140 && p.g > p.b.saturating_add(40)
                } else {
                    p.r > 90
                }
            })
            .count()
    };
    assert_eq!(icon_pixels(&render(), false), 0);
    hover(at);
    assert!(icon_pixels(&render(), false) > 4);
    click();
    assert!(icon_pixels(&render(), true) > 4);
    advance(1010);
    click();
    hover(LogicalPosition::new(10., 400.));
    advance(2500);
    assert!(icon_pixels(&render(), true) > 4);
    advance(3015);
    assert_eq!(icon_pixels(&render(), false), 0);
    hover(at);
    assert!(icon_pixels(&render(), false) > 4);
    assert_eq!(icon_pixels(&render(), true), 0);
}

#[test]
fn hovering_sort_headings_brightens_only_the_text() {
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: Rc::new(Cell::new(0)),
    }))
    .unwrap();
    let ui = RoomWindow::new().unwrap();
    ui.set_joined(true);
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    let render = || {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut pixels, 388);
        });
        pixels
    };
    let before = render();
    for (left, right) in [(64, 173), (173, 237)] {
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: LogicalPosition::new(left as f32 + 10., 115.),
        });
        let after = render();
        let mut lighter = 0;
        for y in 107..126 {
            for x in left..right {
                let i = y * 388 + x;
                if x >= right - 12 {
                    assert_eq!(before[i], after[i], "Фон заголовка при x={x}, y={y}");
                } else if after[i].r > before[i].r {
                    lighter += 1;
                }
            }
        }
        assert!(
            lighter > 10,
            "Текст заголовка должен светлеть при наведении"
        );
    }
}

#[test]
fn title_bar_power_reflects_health_and_can_turn_the_client_back_on() {
    use slint::platform::PointerEventButton;
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: Rc::new(Cell::new(0)),
    }))
    .unwrap();
    let ui = RoomWindow::new().unwrap();
    let weak = ui.as_weak();
    ui.on_change_option(move |name, value| {
        assert_eq!(name, "power");
        weak.unwrap().set_power_on(value);
    });
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    let verify_color = |color| {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut pixels, 388);
        });
        assert!(
            (17..44)
                .flat_map(|y| (202..229).map(move |x| y * 388 + x))
                .any(|i| {
                    let p = pixels[i];
                    (p.r, p.g, p.b) == color
                }),
            "Цвет кнопки {color:?}"
        );
    };
    let click = |x, y| {
        let button = PointerEventButton::Left;
        let position = LogicalPosition::new(x, y);
        ui.window()
            .dispatch_event(WindowEvent::PointerPressed { position, button });
        ui.window()
            .dispatch_event(WindowEvent::PointerReleased { position, button });
        platform::update_timers_and_animations();
    };
    verify_color((191, 236, 112));
    ui.set_health("warning".into());
    verify_color((242, 169, 78));
    ui.set_health("critical".into());
    verify_color((240, 116, 116));
    click(215., 29.);
    assert!(!ui.get_power_on());
    verify_color((145, 150, 159));
    click(215., 29.);
    assert!(ui.get_power_on());
    ui.set_power_on(false);
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(194., 301.),
    });
    let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
    surface.request_redraw();
    surface.draw_if_needed(|r| {
        r.render(&mut pixels, 388);
    });
    assert_eq!(pixels[302 * 388 + 120], pixels[302 * 388 + 90]);
    for y in [294., 332.] {
        ui.set_power_on(false);
        click(194., y);
        assert!(ui.get_power_on(), "Клик по значку или подписи при y={y}");
    }
}

#[test]
fn settings_switch_highlights_on_hover_and_toggles_once_per_activation() {
    use slint::platform::{Key, PointerEventButton};
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: Rc::new(Cell::new(0)),
    }))
    .unwrap();
    let ui = RoomWindow::new().unwrap();
    ui.set_settings_open(true);
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let weak = ui.as_weak();
    ui.on_change_option(move |name, value| {
        assert_eq!(name, "autoconnect");
        count.set(count.get() + 1);
        weak.unwrap().set_autoconnect(value);
    });
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    let rail_color = || {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut pixels, 388);
        });
        let color = pixels[296 * 388 + 348];
        (color.r, color.g, color.b)
    };
    let idle = rail_color();
    let position = LogicalPosition::new(334., 296.);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    assert_ne!(rail_color(), idle);
    let button = PointerEventButton::Left;
    ui.window()
        .dispatch_event(WindowEvent::PointerPressed { position, button });
    ui.window()
        .dispatch_event(WindowEvent::PointerReleased { position, button });
    assert!(ui.get_autoconnect());
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Space.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Space.into(),
    });
    assert!(!ui.get_autoconnect());
    assert_eq!(calls.get(), 2);
}

#[test]
fn room_buttons_show_background_only_while_hovered() {
    use slint::platform::{Key, PointerEventButton};
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: Rc::new(Cell::new(0)),
    }))
    .unwrap();
    let ui = RoomWindow::new().unwrap();
    ui.set_joined(true);
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    let render = || {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|r| {
            r.render(&mut pixels, 388);
        });
        pixels
    };
    let header = |pixels: &[Rgb8Pixel]| {
        (14..44)
            .flat_map(|y| {
                pixels[y * 388 + 266..y * 388 + 296]
                    .iter()
                    .map(|p| (p.r, p.g, p.b))
            })
            .collect::<Vec<_>>()
    };
    let before = render();
    for (x, y, background) in [
        (204, 25, (21, 22, 24)),
        (237, 25, (21, 22, 24)),
        (336, 25, (21, 22, 24)),
    ] {
        let p = before[y * 388 + x];
        assert_eq!((p.r, p.g, p.b), background, "Фон кнопки в {x},{y}");
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: LogicalPosition::new(x as f32, y as f32),
        });
        let p = render()[y * 388 + x];
        assert_eq!((p.r, p.g, p.b), (56, 59, 64), "Наведение в {x},{y}");
    }
    let position = LogicalPosition::new(281., 28.);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    let p = render()[20 * 388 + 269];
    assert_eq!((p.r, p.g, p.b), (56, 59, 64));
    let button = PointerEventButton::Left;
    ui.window()
        .dispatch_event(WindowEvent::PointerPressed { position, button });
    ui.window()
        .dispatch_event(WindowEvent::PointerReleased { position, button });
    let p = render()[20 * 388 + 269];
    assert_eq!((p.r, p.g, p.b), (56, 59, 64));
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(8., 430.),
    });
    assert!(ui.get_settings_open());
    let settings = render();
    assert!(
        header(&settings).contains(&(191, 236, 112)),
        "Открытый раздел подсвечен"
    );
    let p = settings[67 * 388 + 200];
    assert_eq!((p.r, p.g, p.b), (21, 22, 24), "Панель профиля скрыта");

    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Tab.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Tab.into(),
    });
    let keyboard = render();
    assert!(
        (13..47).any(|y| (298..332).any(|x| {
            let p = keyboard[y * 388 + x];
            (p.r, p.g, p.b) == (140, 146, 154)
        })),
        "Tab показывает контур следующей кнопки"
    );
    let back = LogicalPosition::new(35., 77.);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: back,
        button,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: back,
        button,
    });
    assert!(!ui.get_settings_open());
    let main = render();
    assert_eq!(header(&main), header(&before));
    let p = main[67 * 388 + 200];
    assert_eq!((p.r, p.g, p.b), (21, 22, 24), "На главном экране нет панели профиля");
}

#[test]
fn update_ring_renders_intermediate_frames_and_stops_at_the_reported_progress() {
    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let time_ms = Rc::new(Cell::new(10u64));
    platform::set_platform(Box::new(TestDesktop { surface: surface.clone(), time_ms: time_ms.clone() })).unwrap();
    let ui = RoomWindow::new().unwrap();
    let update = ui.global::<UpdateUi>();
    update.set_version("0.1.0".into());
    update.set_phase(UpdatePhase::Loading);
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    let frame = |time| {
        time_ms.set(time);
        platform::update_timers_and_animations();
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|renderer| { renderer.render(&mut pixels, 388); });
        (18..40).flat_map(|y| pixels[y*388+139..y*388+165].iter().map(|p| (p.r,p.g,p.b))).collect::<Vec<_>>()
    };
    let empty = frame(20);
    update.set_percent(70);
    let frames: Vec<_> = (0..=12).map(|i| frame(21+i*16)).collect();
    let full = frame(330);
    assert_ne!(empty, full);
    assert!(frames.windows(2).filter(|w| w[0]!=w[1]).count() >= 8, "Загрузка перескочила через промежуточные кадры");
    assert_eq!(full, frame(410));
    update.set_phase(UpdatePhase::Verifying);
    let rotating: Vec<_> = (0..12).map(|i| frame(430+i*16)).collect();
    assert!(rotating.windows(2).all(|w| w[0]!=w[1]), "Дуга проверки перестала вращаться");
}

#[test]
fn long_release_notes_scroll_beside_the_text() {
    use slint::platform::PointerEventButton;

    let surface = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    platform::set_platform(Box::new(TestDesktop {
        surface: surface.clone(),
        time_ms: Rc::new(Cell::new(10)),
    }))
    .unwrap();
    let ui = RoomWindow::new().unwrap();
    let update = ui.global::<UpdateUi>();
    update.set_phase(UpdatePhase::Ready);
    update.set_target("0.1.1".into());
    let notes = update_notes::parse(include_str!("../release/notes/v0.1.1.md"));
    update.set_notes(slint::ModelRc::new(slint::VecModel::from(notes)));
    update.set_dialog_open(true);
    ui.show().unwrap();
    surface.set_size(PhysicalSize::new(388, 540));
    let render = || {
        let mut pixels = vec![Rgb8Pixel::default(); 388 * 540];
        surface.request_redraw();
        surface.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, 388);
        });
        pixels
    };
    let thumb = |image: &[Rgb8Pixel]| {
        (135..405).find(|&y| {
            (330..348).any(|x| {
                let p = image[y * 388 + x];
                (p.r, p.g, p.b) == (104, 110, 118)
            })
        })
    };
    let top = render();
    let y = thumb(&top).expect("Long notes should have a thumb");
    let x = (330..348)
        .find(|&x| {
            let p = top[(y + 3) * 388 + x];
            (p.r, p.g, p.b) == (104, 110, 118)
        })
        .unwrap();
    for line in 145..390 {
        for col in x - 7..x - 2 {
            let p = top[line * 388 + col];
            assert_eq!(
                (p.r, p.g, p.b),
                (32, 34, 37),
                "Text overlaps the scrollbar gutter"
            );
        }
    }
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition::new(120., 200.),
        delta_x: 0.,
        delta_y: -52.,
    });
    let after_wheel = render();
    assert!(thumb(&after_wheel).unwrap() > y);
    let button = PointerEventButton::Left;
    let start = LogicalPosition::new(x as f32, (thumb(&after_wheel).unwrap() + 5) as f32);
    let end = LogicalPosition::new(x as f32, 120.);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: start,
        button,
    });
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position: end });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: end,
        button,
    });
    ui.window().dispatch_event(WindowEvent::PointerExited);
    assert_eq!(
        thumb(&render()),
        Some(y),
        "Dragging the handle back to the top"
    );
    update.set_notes(slint::ModelRc::new(slint::VecModel::from(vec![
        ReleaseNote {
            text: "Short note".into(),
            ..Default::default()
        },
    ])));
    assert!(thumb(&render()).is_none());
    let note = ReleaseNote {
        text: "New — Create a network.".into(),
        new_entry: true,
        ..Default::default()
    };
    update.set_notes(slint::ModelRc::from([note.clone()]));
    let starred = render();
    update.set_notes(slint::ModelRc::from([ReleaseNote {
        text: "\u{2003}New — Create a network.".into(),
        new_entry: false,
        ..note
    }]));
    let unmarked = render();
    assert!((160..360).any(|y| (42..52).any(|x| {
        let i = y * 388 + x;
        starred[i].r > 140 && unmarked[i].r < 70
    })), "The New marker should be visible before the text");
}
