// The editable resume header (name, headline, contact, links), shared by resumes and the default template.

import { list, str, type FieldValue, type Parts } from "./api";

export const HEADER_FIELDS = ["name", "headline", "email", "phone", "location", "links"] as const;

/**
 * Show a file's header fields. Fields it doesn't set fall back to `inherited` (the default
 * template) and are styled as such. Returns how many fields are inherited.
 */
export function fillHeader(root: HTMLElement, own: Parts["fields"], inherited: Parts["fields"] = {}): number {
  let count = 0;
  for (const input of root.querySelectorAll<HTMLInputElement>("input[name]")) {
    const mine = str(own[input.name]);
    const fallback = str(inherited[input.name]);
    const isInherited = !mine && !!fallback;
    input.classList.toggle("inherited", isInherited);
    if (isInherited) count++;
    if (document.activeElement !== input) input.value = mine || fallback;
    fitInput(input);
  }
  return count;
}

const measure = document.createElement("canvas").getContext("2d")!;

/** Contact fields sit on one line like the printed header, so each is as wide as its text. */
function fitInput(input: HTMLInputElement) {
  if (!input.closest(".contact")) return;
  const cs = getComputedStyle(input);
  measure.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  input.style.width = `${Math.ceil(measure.measureText(input.value || input.placeholder).width) + 6}px`;
}

/** Call `save` with the field a header input edits, once the user commits it (blur or Enter). */
export function onHeaderEdit(root: HTMLElement, save: (field: string, value: FieldValue) => void) {
  root.addEventListener("input", (e) => fitInput(e.target as HTMLInputElement));
  root.addEventListener("change", (e) => {
    const input = e.target as HTMLInputElement;
    if (!(HEADER_FIELDS as readonly string[]).includes(input.name)) return;
    save(input.name, input.name === "links" ? list(input.value) : input.value.trim());
  });
  root.addEventListener("keydown", (e) => {
    if (e.key === "Enter") (e.target as HTMLElement).blur();
  });
}
