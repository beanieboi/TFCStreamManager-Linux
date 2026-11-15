// Binds an overlay template's {{placeholders}} to /api/overlay (ADR 0011).
// The server adds this script to every overlay page; templates have none of
// their own.
//
// Classes for templates to style with:
//   tfc-empty                    element whose placeholders are all empty
//   tfc-changed                  element whose value just changed, briefly
//   tfc-idle (body)              nothing on air
//   tfc-source-<s>, tfc-format-<f> (body)  e.g. tfc-source-mtfv, tfc-format-race
(() => {
  "use strict";

  const PLACEHOLDER = /\{\{\s*([A-Za-z][A-Za-z0-9_]*)\s*\}\}/g;
  const CHANGED_MS = 600;
  const follow = document.currentScript?.hasAttribute("data-follow") ?? false;

  const textBindings = []; // { node, template, names, element, value }
  const attrBindings = []; // { element, name, template }
  const emptyTargets = new Map(); // element -> text bindings inside it
  const changeTimers = new WeakMap();
  let rendered = false;

  const namesIn = (template) => [...template.matchAll(PLACEHOLDER)].map((m) => m[1]);
  const fill = (template, fields) =>
    template.replace(PLACEHOLDER, (_, name) => fields[name] ?? "");

  function collect() {
    const walker = document.createTreeWalker(document.documentElement, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const element = node.parentElement;
      if (!element || element.closest("script, style")) continue;
      const names = namesIn(node.nodeValue);
      if (!names.length) continue;
      const binding = { node, template: node.nodeValue, names, element, value: null };
      textBindings.push(binding);
      // The element and its ancestors inside <body> can become tfc-empty.
      for (let el = element; el && el !== document.body && document.body.contains(el); el = el.parentElement) {
        if (!emptyTargets.has(el)) emptyTargets.set(el, []);
        emptyTargets.get(el).push(binding);
      }
    }
    for (const element of document.querySelectorAll("*")) {
      for (const attr of [...element.attributes]) {
        if (namesIn(attr.value).length) {
          attrBindings.push({ element, name: attr.name, template: attr.value });
        }
      }
    }
  }

  function flash(element) {
    clearTimeout(changeTimers.get(element));
    element.classList.remove("tfc-changed");
    void element.offsetWidth; // restart a running transition or animation
    element.classList.add("tfc-changed");
    changeTimers.set(element, setTimeout(() => element.classList.remove("tfc-changed"), CHANGED_MS));
  }

  function setBodyClass(prefix, value) {
    for (const cls of [...document.body.classList]) {
      if (cls.startsWith(prefix)) document.body.classList.remove(cls);
    }
    if (value) document.body.classList.add(prefix + value);
  }

  function render(view) {
    const fields = view?.fields ?? {};
    const isEmpty = (name) => (fields[name] ?? "") === "";

    for (const b of textBindings) {
      const value = fill(b.template, fields);
      if (value === b.value) continue;
      b.node.nodeValue = value; // text, never HTML
      if (rendered && b.value !== null) flash(b.element);
      b.value = value;
    }
    for (const b of attrBindings) {
      b.element.setAttribute(b.name, fill(b.template, fields));
    }
    for (const [element, bindings] of emptyTargets) {
      element.classList.toggle("tfc-empty", bindings.every((b) => b.names.every(isEmpty)));
    }

    document.body.classList.toggle("tfc-idle", !view);
    setBodyClass("tfc-source-", view?.source);
    setBodyClass("tfc-format-", view?.format);
    rendered = view !== null && view !== undefined;
  }

  collect();
  render(null); // no raw {{placeholders}} before the first event

  // EventSource reconnects on its own and gets the current view first.
  const source = new EventSource("/api/overlay");
  source.addEventListener("overlay", (e) => render(JSON.parse(e.data)));
  source.addEventListener("reload", () => {
    if (follow) location.reload();
  });
  // A template file or this script was edited on the server.
  source.addEventListener("templates", () => location.reload());
})();
