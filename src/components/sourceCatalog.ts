// Built-in descriptions shown in the Add Source dialog. Keys are libobs input type ids
// without their version suffix (e.g. "text_gdiplus_v3" -> "text_gdiplus").

export type Category = "Capture" | "Media" | "Audio" | "Web & Text" | "Other";
export const CATEGORIES: Category[] = ["Capture", "Media", "Audio", "Web & Text", "Other"];

export interface SourceDoc {
  category: Category;
  icon: string;
  summary: string;
  tips: string[];
}

const DOCS: Record<string, SourceDoc> = {
  game_capture: {
    category: "Capture", icon: "🎮",
    summary: "Captures a full-screen or windowed game directly from the GPU. The lightest and smoothest way to show a game.",
    tips: [
      "Pick \"Capture specific window\" so the source follows that game only.",
      "If it stays black (anti-cheat, some launchers), use Window Capture or Display Capture instead.",
      "Run the app as administrator only if the game itself runs as administrator.",
    ],
  },
  window_capture: {
    category: "Capture", icon: "🪟",
    summary: "Captures one application window, even when it is behind other windows.",
    tips: [
      "Use \"Windows 10 (1903 and up)\" capture method for browsers and modern apps that show black.",
      "Minimized windows cannot be captured.",
    ],
  },
  monitor_capture: {
    category: "Capture", icon: "🖥",
    summary: "Captures everything on one monitor, including notifications and private windows.",
    tips: [
      "Anything on that screen goes live: close private messages and passwords first.",
      "On laptops with two GPUs, a black capture usually means the app runs on the wrong GPU.",
    ],
  },
  dshow_input: {
    category: "Capture", icon: "📷",
    summary: "Webcam, capture card or any video capture device.",
    tips: [
      "A device can only be opened by one app at a time; close Discord/Zoom camera preview first.",
      "Set resolution and FPS to match the device for the lowest CPU use.",
      "Capture cards: enable \"Use custom audio device\" or add its audio in the mixer.",
    ],
  },
  "decklink-input": {
    category: "Capture", icon: "🎞",
    summary: "Blackmagic DeckLink / Intensity capture card input.",
    tips: ["Requires Blackmagic Desktop Video drivers."],
  },
  aja_source: {
    category: "Capture", icon: "🎞",
    summary: "AJA video I/O card input.",
    tips: ["Requires AJA drivers."],
  },
  image_source: {
    category: "Media", icon: "🖼",
    summary: "A still image or animated GIF: logos, frames, backgrounds.",
    tips: ["PNG keeps transparency. Very large images use more video memory."],
  },
  slideshow: {
    category: "Media", icon: "🗂",
    summary: "Cycles through a list of images with a transition.",
    tips: ["Good for sponsor rotations and \"starting soon\" screens."],
  },
  ffmpeg_source: {
    category: "Media", icon: "🎬",
    summary: "Plays a video or audio file, or a network stream URL.",
    tips: [
      "Enable \"Restart playback when source becomes active\" for intros and stingers.",
      "Hardware decoding lowers CPU use for large videos.",
    ],
  },
  vlc_source: {
    category: "Media", icon: "📼",
    summary: "Playlist player based on VLC.",
    tips: ["Requires VLC (64-bit) installed on this PC."],
  },
  color_source: {
    category: "Media", icon: "🟦",
    summary: "A solid color rectangle: backgrounds, bars, simple masks.",
    tips: ["Costs almost nothing to render."],
  },
  wasapi_input_capture: {
    category: "Audio", icon: "🎙",
    summary: "An extra microphone or line input (the main mic lives in Settings → Audio).",
    tips: [
      "Opening a Bluetooth headset mic drops the headset to hands-free quality.",
      "Add Noise Suppression and a Noise Gate filter for a cleaner voice.",
    ],
  },
  wasapi_output_capture: {
    category: "Audio", icon: "🔊",
    summary: "Captures everything played on an output device (speakers, headset).",
    tips: ["Main desktop audio is already in the mixer; add this only for a second device."],
  },
  wasapi_process_output_capture: {
    category: "Audio", icon: "🎧",
    summary: "Captures audio from a single application, e.g. only the game or only Spotify.",
    tips: [
      "Lets you keep Discord or music off stream while the game stays on.",
      "If you also have Desktop Audio, that app will be heard twice; mute one of them.",
    ],
  },
  browser_source: {
    category: "Web & Text", icon: "🌐",
    summary: "A web page rendered on stream: alerts, chat overlay, widgets, timers.",
    tips: [
      "Each browser source runs its own web page; keep only the ones you need.",
      "Enable \"Shutdown source when not visible\" to save CPU on hidden scenes.",
      "Chat overlay URL: Settings → Chat.",
    ],
  },
  text_gdiplus: {
    category: "Web & Text", icon: "🔤",
    summary: "Text with font, color, outline and scrolling; can read text from a file.",
    tips: ["\"Read from file\" lets other tools update the text live (now playing, counters)."],
  },
  text_ft2_source: {
    category: "Web & Text", icon: "🔠",
    summary: "Cross-platform text renderer (FreeType).",
    tips: ["Prefer Text (GDI+) on Windows unless you need this one's rendering."],
  },
};

const baseId = (id: string) => id.replace(/_v\d+$/, "");

export function sourceDoc(typeId: string, hasVideo: boolean, hasAudio: boolean): SourceDoc {
  return DOCS[baseId(typeId)] ?? {
    category: "Other",
    icon: hasVideo ? (hasAudio ? "🎬" : "🖼") : "🔊",
    summary: "Provided by an installed plug-in or module. Its settings appear after adding it.",
    tips: [],
  };
}

/** Order inside a category: the ones Streamlabs users look for first. */
export const FEATURED = Object.keys(DOCS);
export const featuredRank = (typeId: string) => {
  const i = FEATURED.indexOf(baseId(typeId));
  return i < 0 ? 999 : i;
};
