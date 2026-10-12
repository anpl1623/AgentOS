import { describe, expect, it } from "vitest";

import { sanitiseForClipboard } from "./hooks";

const ESC = "\u001b";
const BEL = "\u0007";

describe("sanitiseForClipboard", () => {
  it("leaves ordinary text, tabs and newlines alone", () => {
    const text = "total 8\n-rw-r--r--\t1 me  staff  42 notes.txt\nünïcödé → ok";
    expect(sanitiseForClipboard(text)).toBe(text);
  });

  it("removes colour and cursor sequences whole, parameters included", () => {
    expect(sanitiseForClipboard(`${ESC}[1;31merror${ESC}[0m: ${ESC}[2K${ESC}[1Adone`)).toBe(
      "error: done",
    );
  });

  it("removes a screen clear and a full reset", () => {
    expect(sanitiseForClipboard(`a${ESC}[2J${ESC}[Hb${ESC}cc`)).toBe("abc");
  });

  it("removes OSC sequences with either terminator", () => {
    expect(sanitiseForClipboard(`${ESC}]0;owned${BEL}x`)).toBe("x");
    expect(
      sanitiseForClipboard(`${ESC}]8;;https://evil.example${ESC}\\link${ESC}]8;;${ESC}\\`),
    ).toBe("link");
  });

  it("cannot end a bracketed paste early", () => {
    const payload = `harmless${ESC}[201~rm -rf ~\n`;
    const clean = sanitiseForClipboard(payload);
    expect(clean).not.toContain(ESC);
    expect(clean).not.toContain("[201~");
  });

  it("removes the 8-bit C1 forms of CSI and OSC", () => {
    expect(sanitiseForClipboard("a\u009b31mb\u009d0;t\u009cc")).toBe("abc");
  });

  it("drops carriage returns, so pasted text cannot overwrite itself", () => {
    expect(sanitiseForClipboard("safe\rrm -rf /\r\n")).toBe("saferm -rf /\n");
  });

  it("drops every other C0 and C1 control and DEL", () => {
    let controls = "";
    for (let code = 0; code <= 0x9f; code += 1) {
      if (code < 0x20 || code >= 0x7f) controls += String.fromCharCode(code);
    }
    expect(sanitiseForClipboard(`[${controls}]`)).toBe("[\t\n]");
  });

  it("leaves a stray escape inert rather than swallowing what follows", () => {
    const clean = sanitiseForClipboard(`${ESC}]0;never terminated, still visible`);
    expect(clean).not.toContain(ESC);
    expect(clean).toContain("still visible");
  });
});
