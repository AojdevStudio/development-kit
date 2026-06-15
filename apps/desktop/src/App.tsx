import { resolveShellLayout } from "./shell/resolveShellLayout";
import { shellConfig } from "./shell/config";
import { productRegistry } from "./shell/productRegistry";
import { Shell } from "./shell/Shell";

/**
 * App entry. Resolves the shell layout once from the kit's shell config plus the
 * registered products (issue #63), then renders the shell.
 *
 * A multi-product app leaves `shellConfig.primaryProduct` unset and gets the
 * generic multi-product shell. A single-product app (e.g. OrinSync) sets
 * `shellConfig.primaryProduct` to its product's namespace, and that product owns
 * the root surface — no foundation fork required. The product-module seam is
 * unchanged either way; this is pure config + presentation.
 */
export function App() {
  const layout = resolveShellLayout(shellConfig, productRegistry);
  return <Shell layout={layout} />;
}
