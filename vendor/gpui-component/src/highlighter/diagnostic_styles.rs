use gpui::{App, Hsla};
use gpui_base::input::DiagnosticSeverity;

use crate::ActiveTheme as _;

pub(crate) fn diagnostic_foreground(severity: DiagnosticSeverity, cx: &App) -> Hsla {
    let status = &cx.theme().highlight_theme.style.status;
    match severity {
        DiagnosticSeverity::Error => status.error(cx),
        DiagnosticSeverity::Warning => status.warning(cx),
        DiagnosticSeverity::Info => status.info(cx),
        DiagnosticSeverity::Hint => status.hint(cx),
    }
}
