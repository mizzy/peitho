interface ScreenDetailed {
  availLeft: number;
  availTop: number;
  availWidth: number;
  availHeight: number;
}

interface ScreenDetails {
  currentScreen: ScreenDetailed;
  screens: readonly ScreenDetailed[];
}

interface FullscreenOptions {
  screen?: ScreenDetailed;
}

interface Window {
  getScreenDetails?: () => Promise<ScreenDetails>;
}
