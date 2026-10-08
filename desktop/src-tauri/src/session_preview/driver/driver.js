// The session preview driver: what `bee preview click|type|press|scroll|
// snapshot|wait-for` does inside the page.
//
// It runs in the isolated WKContentWorld `beekeeper-preview-driver` of the
// preview webview, never in the page's own world: it shares the page's DOM
// but none of its globals, so the page can neither see it nor spoof it.
// Rust (`driver.rs`) prepends Playwright's vendored injected script
// (desktop/vendor/playwright-injected) and calls `run(op)` through
// `callAsyncJavaScript`, which awaits the returned promise and reports a
// throw as an error.
//
// Every input it produces is synthetic (`isTrusted === false`) and every
// result says so. Native widgets that only trusted input reaches (file
// pickers, a native <select> popup, the clipboard) are out of reach, which
// is disclosed to the agent rather than papered over.
//
// Results are plain objects: `{ ok: true, ... }` or
// `{ ok: false, code, message }`, where `code` is a WIRE-C4 refusal code.

globalThis.__beekeeperPreviewDriverFactory = (createInjected) => {
  let injected = null;
  let snapshotUrl = null;

  const pw = () => {
    if (!injected) injected = createInjected();
    return injected;
  };

  const fail = (code, message, extra) => ({
    ok: false,
    code,
    message,
    ...(extra || {}),
  });

  // Playwright's attribute-selector escaping (`escapeForAttributeSelector`).
  const attr = (value, exact) =>
    `"${String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"${exact ? "s" : "i"}`;
  // Playwright's text-selector escaping (`escapeForTextSelector`).
  const text = (value, exact) =>
    `${JSON.stringify(String(value))}${exact ? "s" : "i"}`;

  const FAMILIES = [
    "ref",
    "role",
    "label",
    "text",
    "placeholder",
    "testId",
    "selector",
  ];

  // A WIRE-C4 locator -> a Playwright selector string.
  const selectorFor = (target) => {
    const present = FAMILIES.filter(
      (key) => target[key] !== undefined && target[key] !== null,
    );
    if (present.length !== 1) {
      return {
        error: fail(
          "preview_bad_request",
          `A target needs exactly one of ${FAMILIES.join(", ")}; got ${present.length}.`,
        ),
      };
    }
    const exact = target.exact === true;
    switch (present[0]) {
      case "ref":
        return { ref: true, selector: `aria-ref=${target.ref}` };
      case "role":
        return {
          selector:
            `internal:role=${target.role}` +
            (target.name !== undefined && target.name !== null
              ? `[name=${attr(target.name, exact)}]`
              : ""),
        };
      case "label":
        return { selector: `internal:label=${text(target.label, exact)}` };
      case "text":
        return { selector: `internal:text=${text(target.text, exact)}` };
      case "placeholder":
        return {
          selector: `internal:attr=[placeholder=${attr(target.placeholder, exact)}]`,
        };
      case "testId":
        return {
          selector: `internal:testid=[data-testid=${attr(target.testId, true)}]`,
        };
      default:
        return { selector: String(target.selector) };
    }
  };

  // Resolve a locator to one element, or a refusal.
  const resolve = (target, generation) => {
    if (!target || typeof target !== "object") {
      return { error: fail("preview_bad_request", "A target is required.") };
    }
    let found = selectorFor(target);
    if (found.error) return found;
    if (found.ref) {
      const match = /^(e\d+|f\d+e\d+)@g(\d+)$/.exec(String(target.ref));
      if (
        !match ||
        Number(match[2]) !== generation ||
        snapshotUrl !== location.href
      ) {
        return {
          error: fail(
            "preview_stale_ref",
            "That ref is from an earlier page; take a new snapshot.",
          ),
        };
      }
      found = { ref: true, selector: `aria-ref=${match[1]}` };
    }
    let elements;
    try {
      const engine = pw();
      elements = engine.querySelectorAll(
        engine.parseSelector(found.selector),
        document,
      );
    } catch (error) {
      return {
        error: fail(
          "preview_bad_request",
          `That target is not a valid locator: ${error?.message}`,
        ),
      };
    }
    if (elements.length === 0) {
      return {
        error: fail(
          found.ref ? "preview_stale_ref" : "preview_target_not_found",
          found.ref
            ? "That ref is from an earlier page; take a new snapshot."
            : "Nothing on the page matches that target.",
        ),
      };
    }
    if (typeof target.nth === "number") {
      const element = elements[target.nth];
      if (!element) {
        return {
          error: fail(
            "preview_target_not_found",
            `Nothing on the page matches that target at nth ${target.nth} (${elements.length} matched).`,
          ),
        };
      }
      return { element };
    }
    if (elements.length > 1) {
      return {
        error: fail(
          "preview_target_ambiguous",
          `That target matches ${elements.length} elements; add --nth or narrow it.`,
          { count: elements.length },
        ),
      };
    }
    return { element: elements[0] };
  };

  const center = (element) => {
    element.scrollIntoView({ block: "center", inline: "center" });
    const box = element.getBoundingClientRect();
    return { x: box.left + box.width / 2, y: box.top + box.height / 2, box };
  };

  const isVisible = (element) => pw().utils.isElementVisible(element);

  const BUTTONS = { left: 0, middle: 1, right: 2 };

  const click = (op) => {
    const found = resolve(op.target, op.generation);
    if (found.error) return found.error;
    const element = found.element;
    const { x, y } = center(element);
    if (!isVisible(element)) {
      return fail(
        "preview_target_not_found",
        "That target is on the page but not visible.",
      );
    }
    const button = BUTTONS[op.button || "left"];
    if (button === undefined) {
      return fail("preview_bad_request", `Unknown button ${op.button}.`);
    }
    const count = Math.max(1, Math.min(3, op.clickCount || 1));
    const base = {
      bubbles: true,
      cancelable: true,
      composed: true,
      clientX: x,
      clientY: y,
      button,
      buttons: 1 << button,
      view: window,
    };
    // Hit-test like a pointer would: the event goes to what is on top.
    const hit = document.elementFromPoint(x, y);
    const receiver = hit && element.contains(hit) ? hit : element;
    for (let detail = 1; detail <= count; detail++) {
      receiver.dispatchEvent(
        new PointerEvent("pointerdown", {
          ...base,
          detail,
          pointerType: "mouse",
          isPrimary: true,
        }),
      );
      receiver.dispatchEvent(new MouseEvent("mousedown", { ...base, detail }));
      if (detail === 1 && typeof element.focus === "function") {
        element.focus({ preventScroll: true });
      }
      receiver.dispatchEvent(
        new PointerEvent("pointerup", {
          ...base,
          detail,
          buttons: 0,
          pointerType: "mouse",
          isPrimary: true,
        }),
      );
      receiver.dispatchEvent(
        new MouseEvent("mouseup", { ...base, detail, buttons: 0 }),
      );
      if (button === 0) {
        // `click()` runs default actions (links, checkboxes, form submit)
        // that a dispatched MouseEvent does not.
        if (detail === 1) receiver.click();
        else
          receiver.dispatchEvent(
            new MouseEvent("click", { ...base, detail, buttons: 0 }),
          );
      } else if (button === 1) {
        receiver.dispatchEvent(
          new MouseEvent("auxclick", { ...base, detail, buttons: 0 }),
        );
      }
    }
    if (count === 2 && button === 0) {
      receiver.dispatchEvent(
        new MouseEvent("dblclick", { ...base, detail: 2, buttons: 0 }),
      );
    }
    if (button === 2) {
      receiver.dispatchEvent(
        new MouseEvent("contextmenu", { ...base, buttons: 0 }),
      );
    }
    return { ok: true, clicked: true };
  };

  const isTextField = (element) =>
    element instanceof HTMLTextAreaElement ||
    (element instanceof HTMLInputElement &&
      ![
        "checkbox",
        "radio",
        "button",
        "submit",
        "reset",
        "file",
        "image",
      ].includes(element.type));

  // Insert one piece of text where the caret is, the way typing does:
  // `insertText` fires beforeinput/input and works with React's value
  // tracking; the setter path is the fallback for fields it refuses.
  const insert = (element, chunk) => {
    if (document.execCommand("insertText", false, chunk)) return;
    if (isTextField(element)) {
      const proto =
        element instanceof HTMLTextAreaElement
          ? HTMLTextAreaElement.prototype
          : HTMLInputElement.prototype;
      const setter = Object.getOwnPropertyDescriptor(proto, "value").set;
      setter.call(element, element.value + chunk);
      element.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          composed: true,
          inputType: "insertText",
          data: chunk,
        }),
      );
    } else if (element.isContentEditable) {
      element.append(document.createTextNode(chunk));
      element.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          composed: true,
          inputType: "insertText",
          data: chunk,
        }),
      );
    }
  };

  const clearField = (element) => {
    if (isTextField(element)) {
      element.select();
      if (!document.execCommand("delete", false)) {
        const proto =
          element instanceof HTMLTextAreaElement
            ? HTMLTextAreaElement.prototype
            : HTMLInputElement.prototype;
        Object.getOwnPropertyDescriptor(proto, "value").set.call(element, "");
        element.dispatchEvent(
          new InputEvent("input", {
            bubbles: true,
            composed: true,
            inputType: "deleteContentBackward",
          }),
        );
      }
    } else if (element.isContentEditable) {
      const range = document.createRange();
      range.selectNodeContents(element);
      const selection = getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.execCommand("delete", false);
    }
  };

  const keyboardEvent = (type, key, modifiers) =>
    new KeyboardEvent(type, {
      key,
      code: codeFor(key),
      bubbles: true,
      cancelable: true,
      composed: true,
      ...modifiers,
    });

  const codeFor = (key) => {
    if (key.length === 1) {
      if (/[a-z]/i.test(key)) return `Key${key.toUpperCase()}`;
      if (/[0-9]/.test(key)) return `Digit${key}`;
      if (key === " ") return "Space";
      return "";
    }
    return key;
  };

  const type = (op) => {
    const found = resolve(op.target, op.generation);
    if (found.error) return found.error;
    const element = found.element;
    center(element);
    if (!isTextField(element) && !element.isContentEditable) {
      return fail(
        "preview_target_not_found",
        "That target is not a text field, so it cannot be typed into.",
      );
    }
    element.focus({ preventScroll: true });
    if (isTextField(element)) {
      const end = element.value.length;
      try {
        element.setSelectionRange(end, end);
      } catch {
        // Some input types (email, number) have no selection API.
      }
    }
    if (op.clear) clearField(element);
    for (const ch of String(op.text ?? "")) {
      element.dispatchEvent(keyboardEvent("keydown", ch, {}));
      element.dispatchEvent(keyboardEvent("keypress", ch, {}));
      insert(element, ch);
      element.dispatchEvent(keyboardEvent("keyup", ch, {}));
    }
    element.dispatchEvent(new Event("change", { bubbles: true }));
    return { ok: true, typed: true };
  };

  const MODIFIERS = {
    Shift: "shiftKey",
    Control: "ctrlKey",
    Alt: "altKey",
    Meta: "metaKey",
    ControlOrMeta: "metaKey",
  };

  const KEY_ALIASES = {
    Esc: "Escape",
    Return: "Enter",
    Space: " ",
    Del: "Delete",
    Up: "ArrowUp",
    Down: "ArrowDown",
    Left: "ArrowLeft",
    Right: "ArrowRight",
  };

  // Parse `Meta+Shift+a` into the key and its modifier flags.
  const parseChord = (chord) => {
    const parts = String(chord).split("+");
    // `+` itself, or a chord ending in it (`Shift++`).
    let key = parts.pop();
    if (key === "" && parts.length > 0 && parts[parts.length - 1] === "") {
      parts.pop();
      key = "+";
    }
    const modifiers = {};
    for (const name of parts) {
      const flag = MODIFIERS[name];
      if (!flag) return null;
      modifiers[flag] = true;
    }
    key = KEY_ALIASES[key] || key;
    if (!key) return null;
    return { key, modifiers };
  };

  // The few default actions a synthetic key press cannot trigger on its
  // own; anything else is just the events.
  const keyDefault = (element, key, modifiers) => {
    const anyModifier =
      modifiers.ctrlKey || modifiers.metaKey || modifiers.altKey;
    if (key === "Enter" && !anyModifier) {
      if (element instanceof HTMLInputElement && element.form) {
        element.form.requestSubmit();
        return;
      }
      if (element instanceof HTMLTextAreaElement || element.isContentEditable) {
        insert(element, "\n");
        return;
      }
      if (
        element instanceof HTMLButtonElement ||
        element instanceof HTMLAnchorElement
      ) {
        element.click();
      }
      return;
    }
    if (key === " " && !anyModifier) {
      if (
        element instanceof HTMLButtonElement ||
        (element instanceof HTMLInputElement &&
          ["checkbox", "radio", "button", "submit"].includes(element.type))
      ) {
        element.click();
        return;
      }
    }
    if (key === "Backspace" || key === "Delete") {
      document.execCommand(
        key === "Backspace" ? "delete" : "forwardDelete",
        false,
      );
      return;
    }
    if (key === "a" && (modifiers.metaKey || modifiers.ctrlKey)) {
      if (isTextField(element)) element.select();
      else document.execCommand("selectAll", false);
      return;
    }
    if (key === "Tab") {
      const focusables = [
        ...document.querySelectorAll(
          'a[href],button,input,select,textarea,[tabindex]:not([tabindex="-1"]),[contenteditable="true"]',
        ),
      ].filter((el) => !el.disabled && isVisible(el));
      const index = focusables.indexOf(element);
      const next =
        focusables[
          (index + (modifiers.shiftKey ? -1 : 1) + focusables.length) %
            focusables.length
        ];
      if (next) next.focus();
      return;
    }
    if (
      key.length === 1 &&
      !anyModifier &&
      (isTextField(element) || element.isContentEditable)
    ) {
      insert(element, key);
    }
  };

  const press = (op) => {
    const chord = parseChord(op.key);
    if (!chord) {
      return fail("preview_bad_request", `Unknown key ${op.key}.`);
    }
    let element = document.activeElement || document.body;
    if (op.target) {
      const found = resolve(op.target, op.generation);
      if (found.error) return found.error;
      element = found.element;
      element.focus({ preventScroll: true });
    }
    const { key, modifiers } = chord;
    const down = keyboardEvent("keydown", key, modifiers);
    const proceed = element.dispatchEvent(down);
    if (proceed && key.length === 1) {
      element.dispatchEvent(keyboardEvent("keypress", key, modifiers));
    }
    if (proceed) keyDefault(element, key, modifiers);
    element.dispatchEvent(keyboardEvent("keyup", key, modifiers));
    return { ok: true, pressed: true };
  };

  const scroll = (op) => {
    let scroller = document.scrollingElement || document.documentElement;
    if (op.target) {
      const found = resolve(op.target, op.generation);
      if (found.error) return found.error;
      scroller = found.element;
    }
    const isWindow = scroller === document.scrollingElement;
    if (op.to === "top") {
      if (isWindow) window.scrollTo(0, 0);
      else scroller.scrollTop = 0;
    } else if (op.to === "bottom") {
      if (isWindow) window.scrollTo(0, scroller.scrollHeight);
      else scroller.scrollTop = scroller.scrollHeight;
    } else if (op.to !== undefined && op.to !== null) {
      return fail("preview_bad_request", `Unknown scroll target ${op.to}.`);
    } else {
      const dx = Number(op.dx || 0);
      const dy = Number(op.dy || 0);
      scroller.dispatchEvent(
        new WheelEvent("wheel", {
          bubbles: true,
          cancelable: true,
          deltaX: dx,
          deltaY: dy,
        }),
      );
      if (isWindow) window.scrollBy(dx, dy);
      else scroller.scrollBy(dx, dy);
    }
    return {
      ok: true,
      scrollX: isWindow ? window.scrollX : scroller.scrollLeft,
      scrollY: isWindow ? window.scrollY : scroller.scrollTop,
    };
  };

  // One check of a wait_for condition. Rust polls this, so a wait survives
  // the navigation that would destroy a long-running script's world.
  const check = (op) => {
    if (
      typeof op.urlIncludes === "string" &&
      !location.href.includes(op.urlIncludes)
    ) {
      return { ok: true, satisfied: false };
    }
    if (typeof op.text === "string" && !op.target) {
      const body = document.body ? document.body.innerText : "";
      if (!body.includes(op.text)) return { ok: true, satisfied: false };
    }
    if (op.target) {
      const state = op.state || "visible";
      const found = resolve(op.target, op.generation);
      if (found.error) {
        if (found.error.code === "preview_target_not_found") {
          return {
            ok: true,
            satisfied: state === "hidden" || state === "detached",
          };
        }
        return found.error;
      }
      const element = found.element;
      if (typeof op.text === "string" && !element.innerText.includes(op.text)) {
        return { ok: true, satisfied: false };
      }
      const visible = isVisible(element);
      const satisfied =
        state === "attached" ||
        (state === "visible" && visible) ||
        (state === "hidden" && !visible);
      return { ok: true, satisfied };
    }
    return { ok: true, satisfied: true };
  };

  const snapshot = (op) => {
    const root = document.body || document.documentElement;
    if (!root) return { ok: true, aria: "", title: document.title };
    const yaml = pw().ariaSnapshot(root, { mode: "ai" });
    snapshotUrl = location.href;
    // Stamp refs with the navigation generation they belong to.
    const aria = yaml.replace(
      /\[ref=((?:f\d+)?e\d+)\]/g,
      (_, ref) => `[ref=${ref}@g${op.generation}]`,
    );
    return { ok: true, aria, title: document.title };
  };

  const VERBS = { click, type, press, scroll, check, snapshot };

  return {
    run(op) {
      const verb = VERBS[op?.verb];
      if (!verb) {
        return fail("preview_bad_request", `Unknown driver op ${op?.verb}.`);
      }
      return verb(op);
    },
  };
};
