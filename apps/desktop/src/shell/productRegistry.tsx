import type { ComponentType } from "react";
import { NotesScreen } from "../products/notes/NotesScreen";
import type { ProductEntitlements } from "../products/notes/notesEntitlements";

/**
 * A product registered with the desktop shell. This is the frontend mirror of the
 * product-module seam (docs/PRODUCT-MODULE-SEAM.md, dim. 2 "Screens"): a product
 * contributes its root screen by ADDING one entry to `productRegistry` — never by
 * editing the shell — exactly as the backend half adds `pub mod <ns>;` and one
 * `invoke_handler!` line. The shell derives its product nav and its single-product
 * root selection from this list (issue #63), so the registry stays additive.
 */
export interface DesktopProduct {
  /** The product's snake_case namespace; mirrors the backend route prefix. */
  namespace: string;
  /** Human label shown in the multi-product nav. */
  title: string;
  /** The product's root screen, rendered standalone when this product is primary. */
  Root: ComponentType;
}

/**
 * A deny-by-default Notes snapshot. Until the desktop fetches the real
 * `ProductEntitlements` from the backend (authored server-side, ADR-0001), the
 * screen grants nothing — the safe direction the seam endorses. The Tauri command
 * and backend route remain the authority regardless of what the screen shows.
 */
const EMPTY_NOTES_ENTITLEMENTS: ProductEntitlements = {
  account_id: "",
  namespace: "notes",
  features: {},
};

/**
 * The products plugged into the desktop shell. A product adds one entry here; the
 * foundation is untouched. Today this is just the Notes sample product (issue #37);
 * a single-product app (OrinSync) registers its product and sets
 * `shell.primaryProduct` to that product's namespace.
 */
export const productRegistry: DesktopProduct[] = [
  {
    namespace: "notes",
    title: "Notes",
    Root: () => <NotesScreen entitlements={EMPTY_NOTES_ENTITLEMENTS} />,
  },
];
