//! A Mermaid diagram drawn by merman as SVG, in the app's colours, and its picture made from
//! that, off the main thread: GPUI loads it as an asset, keyed by its source and its look,
//! kept while a preview shows it and let go, its picture too, once none does.
//!
//! The look is Mermaid's `base` theme with every colour taken from the palette: the nodes a
//! tint of the accent, the lines and rules the text's colour thinned, the notes the colour of a
//! changed file, the charts' slices the colours code is in. So a diagram reads as part of the
//! window, light or dark, and follows the accent a person picked. Its card is a code block's,
//! as the text view paints one, so a diagram sits in the page as the code beside it does.
//!
//! Its labels are measured in the font they're painted in, macOS's Helvetica Neue, which GPUI's
//! SVG renderer finds among the system's fonts: merman's estimated widths are narrower than
//! the text, and the SVG cuts each label to its measured box.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use gpui_kit::base::text::editor_host::RenderContext;
use gpui_kit::{
    AbsoluteLength, App, Asset, CornersRefinement, Global, Hsla, RenderImage, Rgba, SharedString,
    Window,
};
use merman::svg::SvgPipeline;
use merman::{
    MermaidEngine, OperationControl, RenderOutput, RenderRequest, Renderer,
    SourcePresentationConfig, SvgEnvironment, SvgRequest,
};
use merman_export::{NativeFontContext, NativeFontLimits};
use serde_json::{Map, Value};

use crate::theme::{self, Palette, gray, hex};

/// The family a diagram's labels are in: the system's own San Francisco can't be named to an SVG
/// renderer, and this is the closest that can.
const FAMILY: &str = "Helvetica Neue";
/// Its faces, as macOS has them, read once to measure labels with.
const FONT_FILE: &str = "/System/Library/Fonts/HelveticaNeue.ttc";
/// A diagram's colours and type, as merman's site config: the same look draws the same picture.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Look(Arc<str>);

/// A diagram drawn as SVG, and its size in points.
#[derive(Debug)]
pub struct Svg {
    pub text: String,
    pub width: f32,
    pub height: f32,
}

/// A diagram drawn: its picture, made as GPUI makes an SVG's, and its size in points.
#[derive(Debug)]
pub struct Drawn {
    pub image: Arc<RenderImage>,
    pub width: f32,
    pub height: f32,
}

/// What a diagram's source draws, or why it doesn't.
pub type Drawing = Result<Arc<Drawn>, SharedString>;

/// A diagram: its source, and the look it's drawn in.
pub type Key = (SharedString, Look);

/// The asset a diagram loads as: drawn, and its picture made, on the background executor.
pub enum Diagram {}

impl Asset for Diagram {
    type Source = Key;
    type Output = Drawing;

    fn load(
        (source, look): Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let svg = draw(&source, &look)?;
            let image = (renderer.render_single_frame(svg.text.as_bytes(), 1.))
                .map_err(|why| why.to_string())?;
            Ok(Arc::new(Drawn {
                image,
                width: svg.width,
                height: svg.height,
            }))
        }
    }
}

/// What a diagram's card is painted with: a code block's fill in a text view, made solid over
/// the page as its labels' boxes are painted in it too, and the block's border and corners.
#[derive(Clone, Debug)]
pub struct Card {
    pub paper: Hsla,
    pub border: Hsla,
    pub corners: CornersRefinement<AbsoluteLength>,
}

impl Card {
    /// The card of a diagram in the theme now.
    pub fn of(cx: &App) -> Card {
        let page = theme::palette(cx).floating();
        let context = RenderContext::from_app(cx);
        let style = context.style();
        Card {
            paper: blend(style.code_background(), 1., page),
            border: style.border(),
            corners: style.code_block().corner_radii.clone(),
        }
    }
}

impl Look {
    /// The look of a diagram in `p`'s colours on `paper`, `dark` or light.
    pub fn of(p: Palette, paper: Hsla, dark: bool) -> Look {
        let config = config(p, paper, dark);
        Look(config.to_string().into())
    }
}

/// What's painted the page's own colour: the ground, the boxes of labels on lines, a Gantt chart's
/// alternate sections, the numbers on a sequence's arrows, a class's odd rows, a pie's rims.
const GROUND: &[&str] = &[
    "background",
    "edgeLabelBackground",
    "relationLabelBackground",
    "altSectionBkgColor",
    "attributeBackgroundColorOdd",
    "sequenceNumberColor",
    "pieStrokeColor",
];

/// The nodes' fill, a tint of the accent: a flowchart's, a sequence's actors, a state's, a task's.
const FILL: &[&str] = &[
    "primaryColor",
    "mainBkg",
    "actorBkg",
    "stateBkg",
    "taskBkgColor",
];

/// The nodes' borders, the accent stronger: a flowchart's, an actor's, an activation's, a task's,
/// and the ring of a state diagram's end.
const BORDER: &[&str] = &[
    "primaryBorderColor",
    "nodeBorder",
    "actorBorder",
    "activationBorderColor",
    "taskBorderColor",
    "activeTaskBorderColor",
    "innerEndBackground",
];

/// Every label and title, in the text's colour.
const TEXT: &[&str] = &[
    "primaryTextColor",
    "secondaryTextColor",
    "tertiaryTextColor",
    "nodeTextColor",
    "textColor",
    "titleColor",
    "noteTextColor",
    "actorTextColor",
    "signalTextColor",
    "labelTextColor",
    "loopTextColor",
    "pieTitleTextColor",
    "pieLegendTextColor",
    "classText",
    "labelColor",
    "stateLabelColor",
    "transitionLabelColor",
    "relationLabelColor",
    "taskTextColor",
    "taskTextDarkColor",
    "taskTextOutsideColor",
    "taskTextLightColor",
    "taskTextClickableColor",
];

/// The lines between nodes and their arrowheads, the text's colour thinned.
const LINE: &[&str] = &[
    "lineColor",
    "arrowheadColor",
    "signalColor",
    "specialStateColor",
    "transitionColor",
    "relationColor",
];

/// What sits behind the nodes, the text's colour faint: clusters, label boxes, activations,
/// composite states, a class's even rows, a Gantt chart's sections, its tasks done and days off.
const SURFACE: &[&str] = &[
    "secondaryColor",
    "tertiaryColor",
    "clusterBkg",
    "labelBoxBkgColor",
    "activationBkgColor",
    "altBackground",
    "compositeBackground",
    "compositeTitleBackground",
    "attributeBackgroundColorEven",
    "doneTaskBkgColor",
    "sectionBkgColor",
    "sectionBkgColor2",
    "excludeBkgColor",
];

/// The rules, the text's colour thinned less: the secondary borders, a cluster's, an actor's
/// line, a label box's, a pie's outer rim, a Gantt chart's grid and its done tasks' borders.
const RULE: &[&str] = &[
    "secondaryBorderColor",
    "tertiaryBorderColor",
    "clusterBorder",
    "actorLineColor",
    "labelBoxBorderColor",
    "pieOuterStrokeColor",
    "gridColor",
    "doneTaskBorderColor",
];

/// Mermaid's `base` theme in `p`'s colours on `paper`, with every variable a diagram reads set,
/// so none is derived from a colour Mermaid picked.
fn config(p: Palette, paper: Hsla, dark: bool) -> Value {
    let pick = |light: f32, dark_: f32| if dark { dark_ } else { light };
    let over = |colour: Hsla, amount: f32| hex(blend(colour, amount, paper));
    let fill = over(p.accent, pick(0.11, 0.24));
    let border = over(p.accent, pick(0.55, 0.75));
    let strong = over(p.accent, pick(0.25, 0.4));
    let text = over(p.text, 1.);
    let line = over(p.text, pick(0.55, 0.6));
    let surface = over(p.text, pick(0.04, 0.06));
    let rule = over(p.text, pick(0.18, 0.22));
    let ground = over(paper, 1.);
    let note = over(p.modified, pick(0.15, 0.22));
    let note_border = over(p.modified, 0.55);
    let critical = over(p.danger, pick(0.2, 0.3));
    let critical_border = over(p.danger, 0.8);
    let on_slice = over(gray(0., 1.), 0.82);
    let mut variables = Map::new();
    variables.insert("darkMode".into(), dark.into());
    let mut set = |keys: &[&str], value: &str| {
        for key in keys {
            variables.insert((*key).into(), value.into());
        }
    };
    set(&["fontFamily"], FAMILY);
    set(&["fontSize"], "14px");
    set(GROUND, &ground);
    set(FILL, &fill);
    set(BORDER, &border);
    set(&["activeTaskBkgColor"], &strong);
    set(TEXT, &text);
    set(LINE, &line);
    set(SURFACE, &surface);
    set(RULE, &rule);
    set(&["noteBkgColor"], &note);
    set(&["noteBorderColor"], &note_border);
    set(&["critBkgColor"], &critical);
    set(&["critBorderColor", "todayLineColor"], &critical_border);
    set(&["pieSectionTextColor"], &on_slice);
    set(&["pieStrokeWidth"], "2px");
    set(&["pieOuterStrokeWidth"], "1px");
    set(&["pieOpacity"], "1");
    set(&["pieTitleTextSize"], "17px");
    set(&["pieSectionTextSize"], "13px");
    // The charts' slices, a mind map's branches, a git graph's lanes: the colours code is in,
    // which go together in either theme; a branch underlined in its own colour, whole, where
    // Mermaid would take the slice's inverse.
    let c = p.code;
    let slices = [
        c.function,
        c.string,
        c.number,
        c.keyword,
        c.kind,
        c.tag,
        c.attribute,
        p.accent,
    ];
    for (at, colour) in slices.into_iter().enumerate() {
        let slice = over(colour, pick(0.62, 0.8));
        let keys = [
            format!("pie{}", at + 1),
            format!("cScale{at}"),
            format!("git{at}"),
            format!("fillType{at}"),
        ];
        for key in keys {
            variables.insert(key, Value::from(slice.as_ref()));
        }
        variables.insert(
            format!("cScaleInv{at}"),
            Value::from(over(colour, 1.).as_ref()),
        );
    }
    serde_json::json!({
        "theme": "base",
        "fontFamily": FAMILY,
        "themeVariables": Value::Object(variables),
    })
}

/// `over` at `amount` of its own opacity, painted over solid `under`.
fn blend(over: Hsla, amount: f32, under: Hsla) -> Hsla {
    Hsla::from(Rgba::from(under).blend(Rgba::from(over).opacity(amount)))
}

/// Draws `source` in `look`: its SVG, or what merman said was wrong with it.
pub fn draw(source: &str, look: &Look) -> Result<Svg, SharedString> {
    let config: Value = serde_json::from_str(&look.0).map_err(|why| why.to_string())?;
    let engine =
        MermaidEngine::new().with_site_config(SourcePresentationConfig::from_value(config));
    let renderer = Renderer::new().with_presentation_engine(engine);
    let environment = match fonts() {
        Some(fonts) => (SvgEnvironment::deterministic().with_native_font_context(fonts.clone()))
            .map_err(|why| why.to_string())?,
        None => SvgEnvironment::deterministic(),
    };
    // What GPUI's SVG renderer can paint: labels as SVG text, not HTML.
    let request = SvgRequest {
        environment,
        pipeline: Some(SvgPipeline::resvg_safe()),
        ..SvgRequest::default()
    };
    let output = (renderer.render(RenderRequest::svg(source, OperationControl::new(), request)))
        .map_err(|why| why.to_string())?;
    let RenderOutput::Svg(Some(svg)) = output else {
        return Err("Mermaid doesn't know this kind of diagram.".into());
    };
    let text = svg.svg().to_owned();
    let (width, height) = size(&text).ok_or("The diagram came out with no size.")?;
    Ok(Svg {
        text,
        width,
        height,
    })
}

/// The faces labels are measured in, read the first time a diagram's drawn; none where the
/// system hasn't got them, when merman estimates widths instead.
fn fonts() -> Option<&'static NativeFontContext> {
    static FONTS: OnceLock<Option<NativeFontContext>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let bytes = std::fs::read(FONT_FILE)
                .inspect_err(|why| {
                    log::info!("no {FONT_FILE} to measure diagrams' labels in: {why}");
                })
                .ok()?;
            let limits = NativeFontLimits::default();
            NativeFontContext::from_font_bytes([bytes], limits, &OperationControl::new())
                .inspect_err(|why| {
                    log::warn!("couldn't measure diagrams' labels in {FAMILY}: {why}");
                })
                .ok()
        })
        .as_ref()
}

/// The width and height of the SVG's `viewBox`, its size in points.
fn size(svg: &str) -> Option<(f32, f32)> {
    let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let end = start + svg[start..].find('"')?;
    let numbers: Vec<f32> = (svg[start..end].split([' ', ',']))
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match numbers[..] {
        [_, _, width, height] if width > 0. && height > 0. => Some((width, height)),
        _ => None,
    }
}

/// The diagrams the previews show, by how many show each.
#[derive(Default)]
struct Shown(HashMap<Key, usize>);

impl Global for Shown {}

/// A preview shows `now` where it showed `was`: the diagrams no preview shows any more leave
/// GPUI's caches, their pictures every window's, and those still being drawn stop.
pub fn showing(was: &HashSet<Key>, now: &HashSet<Key>, window: Option<&mut Window>, cx: &mut App) {
    if was == now {
        return;
    }
    let shown = cx.default_global::<Shown>();
    for key in now.difference(was) {
        *shown.0.entry(key.clone()).or_default() += 1;
    }
    let mut gone = Vec::new();
    for key in was.difference(now) {
        if let Some(count) = shown.0.get_mut(key) {
            *count -= 1;
            if *count == 0 {
                shown.0.remove(key);
                gone.push(key.clone());
            }
        }
    }
    let mut window = window;
    for key in gone {
        if let Some(Ok(drawn)) = cx.fetch_asset::<Diagram>(&key) {
            cx.drop_image(drawn.image.clone(), window.as_deref_mut());
        }
        cx.remove_asset::<Diagram>(&key);
    }
}

/// How many diagrams the previews show.
#[cfg(test)]
pub fn shown(cx: &App) -> usize {
    cx.try_global::<Shown>().map_or(0, |shown| shown.0.len())
}
