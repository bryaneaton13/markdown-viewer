// The page works without JavaScript; this only adds screenshot switching.
const appearances = {
  light: {
    image: "screenshots/light.png",
    alt: "Markdown Viewer in light mode with a folder tree and a rendered document.",
    caption: "Light mode · Folder tree and rendered document.",
  },
  dark: {
    image: "screenshots/dark.png",
    alt: "Markdown Viewer in dark mode with find highlighting.",
    caption: "Dark mode · Find highlights matches in the rendered text.",
  },
  split: {
    image: "screenshots/split.png",
    alt: "Two documents side by side in a split, with the Fjord theme.",
    caption: "Split view · Two documents side by side (⌘\\), Fjord theme.",
  },
  synthwave: {
    image: "screenshots/theme-synthwave.png",
    alt: "The Synthwave theme: purple chrome with magenta accents and two tabs.",
    caption: "Synthwave · A fun theme colors icons, the active tab and the status bar.",
    frame: "dark",
  },
  campfire: {
    image: "screenshots/theme-campfire.png",
    alt: "The Campfire theme: warm brown chrome with orange accents and two tabs.",
    caption: "Campfire · Themes never recolor the page itself.",
    frame: "dark",
  },
  lagoon: {
    image: "screenshots/theme-lagoon.png",
    alt: "The Lagoon theme in light mode: teal status bar and blue file icons.",
    caption: "Lagoon · Every theme has light and dark variants.",
  },
  bubblegum: {
    image: "screenshots/theme-bubblegum.png",
    alt: "The Bubblegum theme in light mode: pink chrome and green file icons.",
    caption: "Bubblegum · Pick a theme from the palette button.",
  },
  preview: {
    image: "screenshots/preview-tab.png",
    alt: "A preview tab with an italic title that the next click in the sidebar reuses.",
    caption: "Preview tabs · Click to peek in an italic tab, double-click to keep it.",
  },
};

const picker = document.querySelector(".theme-picker");
const preview = document.querySelector("#reader-preview");
const caption = document.querySelector("#preview-caption");
const frame = document.querySelector(".window-frame");

if (picker && preview && caption && frame) {
  picker.hidden = false;
  picker.addEventListener("click", (event) => {
    const button = event.target.closest("button[data-theme]");
    const appearance = button && appearances[button.dataset.theme];
    if (!appearance) return;
    preview.src = appearance.image;
    preview.alt = appearance.alt;
    caption.textContent = appearance.caption;
    frame.dataset.previewTheme = appearance.frame || (button.dataset.theme === "dark" ? "dark" : "light");
    for (const option of picker.querySelectorAll("button")) {
      option.setAttribute("aria-pressed", String(option === button));
    }
  });
}
