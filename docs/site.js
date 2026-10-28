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
    frame.dataset.previewTheme = button.dataset.theme;
    for (const option of picker.querySelectorAll("button")) {
      option.setAttribute("aria-pressed", String(option === button));
    }
  });
}
