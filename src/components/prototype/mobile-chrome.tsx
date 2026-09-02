/**
 * What the prototype draws its fake rows with, once the chrome itself moved.
 *
 * Everything a phone screen is built from is now the window's, in
 * `src/components/shell/mobile-chrome.tsx`. This file is the re-export the
 * harness reads it through, so that deleting the prototype is still deleting a
 * folder and nothing else.
 */

export {
  BarButton,
  NavBar,
  Row,
  RowSeparator,
  Screen,
  Sheet,
  Stack,
  TextPlaceholder,
} from "@/components/shell/mobile-chrome";
