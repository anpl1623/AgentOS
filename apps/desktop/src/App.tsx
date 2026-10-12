import { RouterProvider } from "./shell/router";
import { Shell } from "./shell/Shell";

/**
 * The application: the router, and the shell it drives.
 *
 * Everything the window draws lives in `shell/`; this file only says how the
 * two fit together, so the order of providers is visible in one place.
 */
export function App() {
  return (
    <RouterProvider>
      <Shell />
    </RouterProvider>
  );
}
