use gpui_kit::*;
actions!(
    markdown_viewer,
    [
        Quit,
        OpenFile,
        OpenFolder,
        ToggleSidebar,
        FocusSidebar,
        FocusReader,
        QuickOpen,
        SearchInFiles,
        CheckForUpdates,
        ToggleAutoUpdates,
        ShowWhatsNew,
        Outline,
        Find,
        NextMatch,
        PrevMatch,
        Escape,
        ToggleSource,
        ToggleProperties,
        ToggleTheme,
        Larger,
        Smaller,
        ResetSize,
        Back,
        Forward,
        Reload,
        CloseWindow,
        Help,
        FullScreen,
        TreeUp,
        TreeDown,
        TreeLeft,
        TreeRight,
        TreeOpen,
        NewTab,
        CloseTab,
        ReopenClosedTab,
        NextTab,
        PrevTab,
        ToggleSplit,
        FocusLeftPane,
        FocusRightPane,
        OpenInNewTab,
        OpenInSplit,
        CloseWorkspace,
        NextWorkspace,
        PrevWorkspace
    ]
);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = markdown_viewer, no_json)]
pub struct OpenPath {
    pub path: std::path::PathBuf,
}
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = markdown_viewer, no_json)]
pub struct SwitchWorkspace {
    pub index: usize,
}
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = markdown_viewer, no_json)]
pub struct OpenSearchHit {
    pub index: usize,
}
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = markdown_viewer, no_json)]
pub struct JumpHeading {
    pub index: usize,
}

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", OpenFile, None),
        KeyBinding::new("cmd-shift-o", OpenFolder, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-ctrl-s", ToggleSidebar, None),
        KeyBinding::new("cmd-1", FocusSidebar, None),
        KeyBinding::new("cmd-2", FocusReader, None),
        KeyBinding::new("cmd-p", QuickOpen, None),
        KeyBinding::new("cmd-k", QuickOpen, None),
        KeyBinding::new("cmd-shift-f", SearchInFiles, None),
        KeyBinding::new("cmd-shift-l", Outline, None),
        KeyBinding::new("cmd-f", Find, None),
        KeyBinding::new("cmd-g", NextMatch, None),
        KeyBinding::new("cmd-shift-g", PrevMatch, None),
        KeyBinding::new("cmd-shift-m", ToggleSource, None),
        KeyBinding::new("cmd-shift-i", ToggleProperties, None),
        KeyBinding::new("cmd-shift-d", ToggleTheme, None),
        KeyBinding::new("cmd-=", Larger, None),
        KeyBinding::new("cmd-+", Larger, None),
        KeyBinding::new("cmd--", Smaller, None),
        KeyBinding::new("cmd-0", ResetSize, None),
        KeyBinding::new("cmd-[", Back, None),
        KeyBinding::new("cmd-]", Forward, None),
        KeyBinding::new("cmd-r", Reload, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-w", CloseWindow, None),
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("cmd-shift-t", ReopenClosedTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("cmd-shift-]", NextTab, None),
        KeyBinding::new("cmd-shift-[", PrevTab, None),
        KeyBinding::new("cmd-}", NextTab, None),
        KeyBinding::new("cmd-{", PrevTab, None),
        KeyBinding::new("cmd-\\", ToggleSplit, None),
        KeyBinding::new("cmd-alt-left", FocusLeftPane, None),
        KeyBinding::new("cmd-alt-right", FocusRightPane, None),
        KeyBinding::new("ctrl-cmd-]", NextWorkspace, None),
        KeyBinding::new("ctrl-cmd-[", PrevWorkspace, None),
        KeyBinding::new("cmd-enter", OpenInNewTab, Some("Command")),
        KeyBinding::new("alt-enter", OpenInSplit, Some("Command")),
        KeyBinding::new("cmd-ctrl-f", FullScreen, None),
        KeyBinding::new("cmd-/", Help, None),
        KeyBinding::new("escape", Escape, Some("Viewer")),
        KeyBinding::new("up", TreeUp, Some("MarkdownTree")),
        KeyBinding::new("down", TreeDown, Some("MarkdownTree")),
        KeyBinding::new("left", TreeLeft, Some("MarkdownTree")),
        KeyBinding::new("right", TreeRight, Some("MarkdownTree")),
        KeyBinding::new("enter", TreeOpen, Some("MarkdownTree")),
        KeyBinding::new("space", FocusReader, Some("MarkdownTree")),
        KeyBinding::new("cmd-enter", OpenInNewTab, Some("MarkdownTree")),
        KeyBinding::new("alt-enter", OpenInSplit, Some("MarkdownTree")),
    ]);
    cx.bind_keys(
        [
            "ctrl-1", "ctrl-2", "ctrl-3", "ctrl-4", "ctrl-5", "ctrl-6", "ctrl-7", "ctrl-8",
            "ctrl-9",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, key)| KeyBinding::new(key, SwitchWorkspace { index }, None)),
    );
    set_menus(true, cx);
}

/// The menu bar. Rebuilt when "Check for Updates Automatically" changes,
/// since the check mark is part of the item.
pub fn set_menus(auto_updates: bool, cx: &mut App) {
    cx.set_menus([
        Menu::new("Markdown Viewer").items([
            MenuItem::action("What's New", ShowWhatsNew),
            MenuItem::action("Check for Updates…", CheckForUpdates),
            MenuItem::action("Check for Updates Automatically", ToggleAutoUpdates)
                .checked(auto_updates),
            MenuItem::separator(),
            MenuItem::action("Keyboard Shortcuts & Setup", Help),
            MenuItem::separator(),
            MenuItem::action("Quit Markdown Viewer", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open Markdown…", OpenFile),
            MenuItem::action("Add Folder Workspace…", OpenFolder),
            MenuItem::action("Quick Open…", QuickOpen),
            MenuItem::separator(),
            MenuItem::action("New Tab", NewTab),
            MenuItem::action("Reopen Closed Tab", ReopenClosedTab),
            MenuItem::separator(),
            MenuItem::action("Reload", Reload),
            MenuItem::action("Close Tab", CloseTab),
            MenuItem::action("Close Workspace", CloseWorkspace),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Copy", gpui_kit::base::input::Copy),
            MenuItem::action("Select All", gpui_kit::base::input::SelectAll),
            MenuItem::action("Find…", Find),
            MenuItem::action("Search in Files…", SearchInFiles),
            MenuItem::action("Next Match", NextMatch),
            MenuItem::action("Previous Match", PrevMatch),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Focus Sidebar", FocusSidebar),
            MenuItem::action("Focus Reader", FocusReader),
            MenuItem::action("Source / Preview", ToggleSource),
            MenuItem::action("Properties", ToggleProperties),
            MenuItem::action("Outline…", Outline),
            MenuItem::action("Toggle Dark Mode", ToggleTheme),
            MenuItem::action("Larger Text", Larger),
            MenuItem::action("Smaller Text", Smaller),
            MenuItem::action("Actual Size", ResetSize),
            MenuItem::action("Full Screen", FullScreen),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Back", Back),
            MenuItem::action("Forward", Forward),
            MenuItem::separator(),
            MenuItem::action("Next Tab", NextTab),
            MenuItem::action("Previous Tab", PrevTab),
            MenuItem::action("Next Workspace", NextWorkspace),
            MenuItem::action("Previous Workspace", PrevWorkspace),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Split Side by Side", ToggleSplit),
            MenuItem::action("Focus Left Pane", FocusLeftPane),
            MenuItem::action("Focus Right Pane", FocusRightPane),
        ]),
    ]);
}
