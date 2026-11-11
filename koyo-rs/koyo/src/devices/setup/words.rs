//! What the setup says: its rows, what to do at each step in a line or three, and the menu of
//! models.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, AnyElement, App, Context, IntoElement as _, ParentElement as _, Styled as _, div};
use koyo_device_wire::XCODE;

use super::{ChooseModel, Install, Setup, Step};
use crate::devices::Standing;
use crate::devices::needs::{Need, Xcode};
use crate::devices::simulators::Model;

/// Each step's name and what it's for.
pub(super) const ROWS: [(&str, &str); 5] = [
    ("Xcode 27 or later", "Apple's tools, which run the simulators. Free in the App Store."),
    ("Xcode's components", "Installed the first time Xcode is used."),
    ("An iOS system", "What the simulators run: about 8 GB, shared with Xcode."),
    ("A device", "An iPhone or iPad of Koyo's own, kept apart from Xcode's."),
    ("First start", "About a minute, once; a start after that takes seconds."),
];

/// Coming back to Koyo is all it takes.
const BACK: &str = "Then come back: this dialog sees it by itself.";

/// What to do at `step`.
pub(super) fn hint(setup: &Setup, step: Step, cx: &App) -> String {
    let found = setup.devices.read(cx).found.as_ref();
    let system = found.and_then(|found| found.systems.first()).map_or("iOS", |system| system.name.as_str());
    let made = setup.made().map(|link| link.read(cx));
    let name = made.map_or("The device", |link| link.name.as_str());
    let model = || setup.model(cx).unwrap_or_else(|| "an iPhone".into());
    match step {
        Step::Looking => "Looking at what this Mac has…".into(),
        Step::Needs(Need::Xcode) => match found.map(|found| &found.xcode) {
            Some(Xcode::Older(version)) => {
                format!("You have Xcode {version}. Update it in the App Store and open it once. {BACK}")
            }
            Some(Xcode::Other { newest, selected }) => {
                let in_use = selected.as_ref().map_or_else(|| "the Command Line Tools are".to_owned(), |version| format!("Xcode {version} is"));
                format!(
                    "Xcode {newest} is installed, but {in_use} the one in use. Open Xcode {newest}, \
                     choose Settings → Locations, and pick Xcode {newest} under Command Line Tools. {BACK}"
                )
            }
            _ => format!("Install Xcode {XCODE} or later from the App Store and open it once. {BACK}"),
        },
        Step::Needs(Need::Components) => match &setup.install {
            Install::Ended(said) => format!(
                "That didn't finish them{}. Open Xcode once and let it install what it asks to, \
                 then come back.",
                if said.is_empty() { String::new() } else { format!(": “{said}”") }
            ),
            _ => "Xcode installs a few components the first time it's used. Continue has Koyo use \
                  it once, here; macOS may ask for your password."
                .into(),
        },
        Step::Installing => "Installing Xcode's components. This can take a few minutes.".into(),
        Step::Needs(Need::System) => format!(
            "Koyo's simulators run iOS, which Xcode downloads, about 8 GB: in Xcode, choose \
             Settings → Components and get iOS {XCODE}. {BACK}"
        ),
        Step::Needs(Need::Device) => format!(
            "Koyo makes {model} on {system} in a set of its own: Xcode's simulators stay as they \
             are. It takes about 2 GB of disk. Choose another below if you like.",
            model = model()
        ),
        Step::Making => format!("Making {}…", model()),
        Step::Starting => match made.map(|link| (link.standing(), link.started_at)) {
            Some((Standing::Starting(Some(at)), since)) => crate::devices::screens::starting(name, at, since),
            _ => format!("Starting {name}…"),
        },
        Step::Ready => format!(
            "{name} is ready, in its tab. Use it there, or ask an agent to with Koyo's device \
             tools: you can take over at any time."
        ),
        Step::Failed => match made.map(super::super::Link::standing) {
            Some(Standing::Failed(why) | Standing::Ended { why, .. }) => why.to_owned(),
            _ => format!("{name} didn't start."),
        },
        Step::Done => "Everything Koyo's simulators need is here. A new Device tab opens one.".into(),
    }
}

/// A menu of `models`, iPhones then iPads, `ticked` ticked; `pick` makes a model's action.
pub fn models_menu(models: &[Model], ticked: Option<&str>, pick: impl Fn(String) -> Box<dyn Action>) -> NativeMenu {
    let mut menu = NativeMenu::new();
    for (index, model) in models.iter().enumerate() {
        if model.ipad && index > 0 && !models[index - 1].ipad {
            menu = menu.separator();
        }
        menu = menu.menu_with_check(model.name.clone(), ticked == Some(model.name.as_str()), pick(model.name.clone()));
    }
    menu
}

/// The device step's menu of models, the newest system's.
pub(super) fn chooser(setup: &Setup, cx: &mut Context<Setup>) -> AnyElement {
    let chosen = setup.model(cx).unwrap_or_default();
    let label = chosen.clone();
    let menu = crate::settings_view::MenuButton::new("device-setup-model", label, move |cx| {
        let devices = crate::devices::devices(cx);
        let models = devices.read(cx).found.as_ref().map_or(&[][..], |found| found.models.as_slice());
        models_menu(models, Some(&chosen), |name| Box::new(ChooseModel(name)))
    });
    let p = crate::theme::palette(cx);
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().text_sm().text_color(p.muted).child("Device"))
        .child(menu)
        .into_any_element()
}
