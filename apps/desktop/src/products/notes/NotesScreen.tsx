import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  PUBLISH_NOTE,
  productFeatureGateState,
  type ProductEntitlements,
} from "./notesEntitlements";

/**
 * The Notes sample product screen (issue #37) — the React surface of the capstone
 * product, contributed through the seam under `products/notes/`.
 *
 * It exercises a FREE local action and a PAID gated action from ONE
 * server-resolved product-entitlements snapshot:
 *
 * 1. Free: create and list notes locally (no gate). The local store is
 *    authoritative for this local product work (ADR-0001).
 * 2. Paid: "Publish to cloud" is gated by the `notes.publish_note` product key.
 *    - React (this component) hides the publish control when the snapshot does not
 *      grant the key — UX only (ADR-0001).
 *    - The Tauri command `request_publish_note` refuses the local action without
 *      the entitlement — the local guard.
 *    - The backend `POST /notes/publish` is the authority, resolving the snapshot
 *      SERVER-SIDE from the caller's token.
 *
 * The snapshot is the one the backend computed; this component never constructs
 * product entitlements of its own. The desktop reads access, it never decides it.
 */

export interface NotesScreenProps {
  /** The server-resolved Notes product entitlements (fetched from the backend). */
  entitlements: ProductEntitlements;
  /** Injectable command invoker so tests can assert the gated path without Tauri. */
  invokeCommand?: typeof invoke;
}

interface LocalNote {
  id: string;
  title: string;
}

export function NotesScreen({ entitlements, invokeCommand = invoke }: NotesScreenProps) {
  const [notes, setNotes] = useState<LocalNote[]>([]);
  const [title, setTitle] = useState("");
  const [published, setPublished] = useState<string | null>(null);
  const [denied, setDenied] = useState<string | null>(null);

  // Free local action: create a note in local state. No gate (ADR-0001: local
  // product work is free; only publishing to the cloud is the paid capability).
  const addNote = () => {
    const trimmed = title.trim();
    if (trimmed === "") return;
    setNotes((prev) => [{ id: `note_${prev.length + 1}`, title: trimmed }, ...prev]);
    setTitle("");
  };

  // UX gate (presentation only). The Tauri command and backend are the real gates.
  const publishVisibility = productFeatureGateState(entitlements, PUBLISH_NOTE);

  // Paid local action: ask the Tauri command, which enforces the same product key
  // against the server-resolved snapshot.
  const publish = () => {
    setDenied(null);
    invokeCommand<string>("request_publish_note", { entitlements })
      .then((payload) => setPublished(payload))
      .catch((err: unknown) => setDenied(err instanceof Error ? err.message : String(err)));
  };

  return (
    <section aria-label="Notes">
      <h2>Notes</h2>
      <div>
        <input
          aria-label="Note title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="New note title"
        />
        <button type="button" onClick={addNote}>
          Add note
        </button>
      </div>
      <ul>
        {notes.map((note) => (
          <li key={note.id}>{note.title}</li>
        ))}
      </ul>
      {publishVisibility === "hidden" ? (
        <p>Publishing notes to the cloud is a paid feature. Upgrade to enable it.</p>
      ) : (
        <button type="button" onClick={publish}>
          Publish to cloud
        </button>
      )}
      {published && <p>Published: {published}</p>}
      {denied && <p role="alert">Publish refused: {denied}</p>}
    </section>
  );
}
