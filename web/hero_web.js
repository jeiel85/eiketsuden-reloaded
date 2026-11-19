// Eiketsuden Reloaded — browser glue for the WebAssembly build.
// Registers a miniquad plugin that gives the game localStorage-backed save
// slots, the wall clock, the URL hash (launch options such as #gallery),
// whether the pointer is over the canvas, lets it dismiss the HTML loading
// overlay once the first frame is up and show a crash message if it panics.
// Requires mq_js_bundle.js (sapp_jsutils helpers js_object/consume_js_object).
//
// Keep in sync with crates/hero-game/src/platform/web.rs. Bump `version`
// below and HERO_WEB_VERSION there together whenever the API changes.
"use strict";
(function () {
  function storage() {
    try {
      return window.localStorage;
    } catch (e) {
      return null; // storage disabled (private mode, sandboxed iframe, ...)
    }
  }

  // Whether the pointer is over the game canvas and the page has focus: miniquad reports no
  // "pointer left" event, so edge scrolling would go on with the last position inside.
  var pointerInside = true;
  function trackPointer() {
    var canvas = document.getElementById("glcanvas");
    if (!canvas) return;
    canvas.addEventListener("mouseleave", function () { pointerInside = false; });
    canvas.addEventListener("mouseenter", function () { pointerInside = true; });
    window.addEventListener("blur", function () { pointerInside = false; });
    // Back from another window without leaving the canvas, no mouseenter comes: the first move
    // over the canvas (with the page focused) turns it back on.
    canvas.addEventListener("mousemove", function () { pointerInside = document.hasFocus(); });
  }

  function register_plugin(importObject) {
    trackPointer();
    // 1 while the pointer is over the canvas and the page has focus, else 0.
    importObject.env.hero_pointer_inside = function () {
      return pointerInside ? 1 : 0;
    };
    importObject.env.hero_web_ready = function () {
      var el = document.getElementById("loading");
      if (el) el.remove();
      var canvas = document.getElementById("glcanvas");
      if (canvas) canvas.focus();
    };
    // The game panicked and cannot continue: show the message over the frozen canvas.
    importObject.env.hero_web_panic = function (message) {
      var text = consume_js_object(message);
      var box = document.getElementById("crash");
      if (!box) {
        box = document.createElement("div");
        box.id = "crash";
        box.setAttribute("role", "alert");
        document.body.appendChild(box);
      }
      box.textContent = "";
      var title = document.createElement("h1");
      title.textContent = "오류가 발생했습니다 / The game crashed";
      var hint = document.createElement("p");
      hint.textContent = "페이지를 새로 고치면 다시 시작합니다. 버그 신고 시 아래 내용을 함께 보내 주세요.";
      var detail = document.createElement("pre");
      detail.textContent = text;
      box.appendChild(title);
      box.appendChild(hint);
      box.appendChild(detail);
    };
    // 1 when the key exists, 0 when it is missing or storage is unavailable.
    importObject.env.hero_storage_has = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      try {
        return s && s.getItem(k) !== null ? 1 : 0;
      } catch (e) {
        console.error("hero_storage_has failed", e);
        return 0;
      }
    };
    // Returns the stored string ("" when missing; check hero_storage_has first).
    importObject.env.hero_storage_get = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      var v = null;
      try {
        v = s ? s.getItem(k) : null;
      } catch (e) {
        console.error("hero_storage_get failed", e);
      }
      return js_object(v === null ? "" : v);
    };
    // Returns 1 on success, 0 when the write failed (quota exceeded / storage disabled).
    importObject.env.hero_storage_set = function (key, value) {
      var k = consume_js_object(key);
      var v = consume_js_object(value);
      var s = storage();
      if (!s) return 0;
      try {
        s.setItem(k, v);
        return 1;
      } catch (e) {
        console.error("hero_storage_set failed", e);
        return 0;
      }
    };
    // Returns 1 on success (also when the key did not exist), 0 when storage is unavailable.
    importObject.env.hero_storage_remove = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      if (!s) return 0;
      try {
        s.removeItem(k);
        return 1;
      } catch (e) {
        console.error("hero_storage_remove failed", e);
        return 0;
      }
    };
    // Wall clock in Unix seconds (fractional), for save timestamps.
    importObject.env.hero_now_seconds = function () {
      return Date.now() / 1000;
    };
    // The URL fragment including '#', e.g. "#gallery" ("" when there is none).
    importObject.env.hero_location_hash = function () {
      return js_object(window.location.hash || "");
    };

    // ----- clipboard ----------------------------------------------------------------------
    // Save export / import through the system clipboard. Both directions are async in the
    // browser, and the read needs a key press or click the page counts as the player's (the game
    // calls these from a key press, so it has one). The game polls hero_clipboard_state until
    // it is no longer 0 and then takes the text with hero_clipboard_text.
    var CLIP_NO_REQUEST = -1;
    var CLIP_RUNNING = 0;
    var CLIP_COPIED = 1;
    var CLIP_COPY_FAILED = 2;
    var CLIP_PASTED = 3;
    var CLIP_EMPTY = 4;
    var CLIP_PASTE_FAILED = 5;
    var clipState = CLIP_NO_REQUEST;
    var clipText = "";
    // The request number the answer belongs to: a newer request ignores an older answer.
    var clipSeq = 0;

    function clipApi() {
      try {
        return navigator.clipboard || null;
      } catch (e) {
        return null; // some sandboxed frames throw on the property itself
      }
    }
    function clipStart() {
      clipSeq += 1;
      clipState = CLIP_RUNNING;
      clipText = "";
      return clipSeq;
    }
    function clipSettle(seq, state, text) {
      if (seq !== clipSeq) return; // superseded by a newer request
      clipState = state;
      clipText = text || "";
    }
    function clipReason(e) {
      if (!e) return "클립보드를 쓰지 못했습니다";
      if (e.name === "NotAllowedError") {
        return "브라우저가 클립보드 사용을 허용하지 않았습니다 (주소창 옆 권한을 확인해 주세요)";
      }
      if (e.name === "SecurityError") {
        return "이 페이지에서는 클립보드를 쓸 수 없습니다 (HTTPS 또는 localhost가 필요합니다)";
      }
      return (e.name ? e.name + ": " : "") + (e.message || "클립보드를 쓰지 못했습니다");
    }

    // Put the text on the clipboard; hero_clipboard_state says how it went.
    importObject.env.hero_clipboard_copy = function (text) {
      var value = consume_js_object(text);
      var api = clipApi();
      if (!api || !api.writeText) {
        clipState = CLIP_COPY_FAILED;
        clipText = "이 브라우저에서는 클립보드를 쓸 수 없습니다";
        return;
      }
      var seq = clipStart();
      var pending;
      try {
        // Some browsers throw synchronously (an unfocused document, a sandboxed frame)
        // instead of rejecting the promise; both must end in a state the game can read.
        pending = api.writeText(value);
      } catch (e) {
        clipSettle(seq, CLIP_COPY_FAILED, clipReason(e));
        return;
      }
      pending.then(
        function () {
          clipSettle(seq, CLIP_COPIED, "");
        },
        function (e) {
          clipSettle(seq, CLIP_COPY_FAILED, clipReason(e));
        }
      );
    };    // Read the clipboard; hero_clipboard_text has the text once hero_clipboard_state says pasted.
    importObject.env.hero_clipboard_paste = function () {
      var api = clipApi();
      if (!api || !api.readText) {
        clipState = CLIP_PASTE_FAILED;
        clipText = "이 브라우저에서는 클립보드를 쓸 수 없습니다";
        return;
      }
      var seq = clipStart();
      var pending;
      try {
        pending = api.readText();
      } catch (e) {
        clipSettle(seq, CLIP_PASTE_FAILED, clipReason(e));
        return;
      }
      pending.then(
        function (value) {
          if (value && value.trim().length > 0) {
            clipSettle(seq, CLIP_PASTED, value);
          } else {
            clipSettle(seq, CLIP_EMPTY, "");
          }
        },
        function (e) {
          clipSettle(seq, CLIP_PASTE_FAILED, clipReason(e));
        }
      );
    };
    // CLIP_NO_REQUEST (no request made yet), CLIP_RUNNING, or the settled state.
    importObject.env.hero_clipboard_state = function () {
      return clipState;
    };
    // The settled request's text (the pasted text, or why it failed).
    importObject.env.hero_clipboard_text = function () {
      return js_object(clipText);
    };
    // Forget the last request (the game keeps the text it took).
    importObject.env.hero_clipboard_reset = function () {
      clipSeq += 1;
      clipState = CLIP_NO_REQUEST;
      clipText = "";
    };
  }

  miniquad_add_plugin({ register_plugin: register_plugin, name: "hero_web", version: 5 });
})();
