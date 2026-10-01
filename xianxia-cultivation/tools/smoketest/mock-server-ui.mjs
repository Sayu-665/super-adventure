// Mock of @minecraft/server-ui: records forms and answers them from a scripted queue.
export const shown = [];
export const answers = []; // queue of selection indices / formValues arrays; empty => canceled

function respond() {
  const a = answers.shift();
  if (a === undefined) return { canceled: true };
  if (Array.isArray(a)) return { canceled: false, formValues: a };
  return { canceled: false, selection: a };
}

class Base {
  constructor(kind) {
    this.kind = kind;
    this.buttons = [];
    this.controls = [];
    this._title = "";
    this._body = "";
  }
  title(t) {
    this._title = String(t);
    return this;
  }
  body(t) {
    this._body = String(t);
    return this;
  }
  show() {
    shown.push(this);
    return Promise.resolve(respond());
  }
}

export class ActionFormData extends Base {
  constructor() {
    super("action");
  }
  button(text, icon) {
    if (icon !== undefined && typeof icon !== "string") throw new Error("icon must be a string path");
    this.buttons.push(String(text));
    return this;
  }
  divider() {
    return this;
  }
  header() {
    return this;
  }
  label() {
    return this;
  }
}

export class MessageFormData extends Base {
  constructor() {
    super("message");
  }
  button1(t) {
    this.buttons[0] = String(t);
    return this;
  }
  button2(t) {
    this.buttons[1] = String(t);
    return this;
  }
}

export class ModalFormData extends Base {
  constructor() {
    super("modal");
  }
  dropdown(label, items, opts) {
    this.controls.push(["dropdown", label, items, opts]);
    return this;
  }
  toggle(label, opts) {
    this.controls.push(["toggle", label, opts]);
    return this;
  }
  slider(label, min, max, opts) {
    this.controls.push(["slider", label, min, max, opts]);
    return this;
  }
  textField(label, ph, opts) {
    this.controls.push(["text", label, ph, opts]);
    return this;
  }
}
