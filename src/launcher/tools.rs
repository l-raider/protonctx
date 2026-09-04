//! Built-in Wine tools launchable in a prefix.

/// A built-in Wine tool, with a display label and the argument passed to `proton runinprefix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTool {
    pub label: &'static str,
    pub arg: &'static str,
}

/// The default set of built-in tools offered for every selected game.
pub const BUILTIN_TOOLS: &[BuiltinTool] = &[
    BuiltinTool { label: "winecfg", arg: "winecfg" },
    BuiltinTool { label: "Task Manager", arg: "taskmgr" },
    BuiltinTool { label: "Explorer", arg: "explorer" },
    BuiltinTool { label: "Registry Editor", arg: "regedit" },
];
