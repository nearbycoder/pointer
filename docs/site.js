// Pointer's website: the hero's sound button, the pointer picker, clips that play while
// they're on screen, and the latest release's version from GitHub.
(() => {
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // The navigation bar gets a hairline once the page scrolls.
  const nav = document.getElementById("nav");
  const onScroll = () => nav.classList.toggle("scrolled", window.scrollY > 8);
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  // The hero film: muted until asked; sound restarts it from the top.
  const promo = document.getElementById("promo");
  const sound = document.getElementById("sound");
  if (reduce) { promo.removeAttribute("autoplay"); promo.pause(); promo.controls = true; }
  sound.addEventListener("click", () => {
    const on = promo.muted;
    promo.muted = !on;
    sound.setAttribute("aria-pressed", String(on));
    sound.querySelector("span").textContent = on ? "Sound off" : "Sound on";
    if (on) { promo.currentTime = 0; promo.play().catch(() => {}); }
  });

  // Clips play while they're on screen, and wait otherwise.
  const clips = [...document.querySelectorAll("video[data-clip], #style-video")];
  if (!reduce && "IntersectionObserver" in window) {
    const seen = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (e.isIntersecting) e.target.play().catch(() => {});
        else e.target.pause();
      }
    }, { threshold: 0.35 });
    clips.forEach((v) => seen.observe(v));
  } else {
    clips.forEach((v) => { v.controls = true; });
  }

  // The pointer picker.
  const stage = document.getElementById("style-video");
  const tabs = [...document.querySelectorAll(".picker-list [role=tab]")];
  const pick = (tab) => {
    if (tab.getAttribute("aria-selected") === "true") return;
    tabs.forEach((t) => t.setAttribute("aria-selected", String(t === tab)));
    const name = tab.dataset.style;
    stage.classList.add("fading");
    setTimeout(() => {
      stage.poster = `media/style-${name}.jpg`;
      stage.src = `media/style-${name}.mp4`;
      stage.setAttribute("aria-label", tab.dataset.label);
      stage.addEventListener("loadeddata", () => stage.classList.remove("fading"), { once: true });
      if (!reduce) stage.play().catch(() => {});
      else stage.classList.remove("fading");
    }, 160);
  };
  tabs.forEach((tab, i) => {
    tab.tabIndex = i === 0 ? 0 : -1;
    tab.addEventListener("click", () => pick(tab));
    tab.addEventListener("keydown", (e) => {
      const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
      if (!step) return;
      e.preventDefault();
      const next = tabs[(i + step + tabs.length) % tabs.length];
      tabs.forEach((t) => (t.tabIndex = t === next ? 0 : -1));
      next.focus();
      pick(next);
    });
  });

  // Sections ease in as they arrive.
  const reveals = document.querySelectorAll(".reveal");
  if (!reduce && "IntersectionObserver" in window) {
    const arrive = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (e.isIntersecting) { e.target.classList.add("in"); arrive.unobserve(e.target); }
      }
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.08 });
    reveals.forEach((el) => arrive.observe(el));
  } else {
    reveals.forEach((el) => el.classList.add("in"));
  }

  // The latest release: its version, date and size, next to the download buttons.
  fetch("https://api.github.com/repos/nearbycoder/pointer/releases/latest", { headers: { Accept: "application/vnd.github+json" } })
    .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
    .then((release) => {
      const version = String(release.tag_name || "").replace(/^v/, "");
      if (!version) return;
      const dmg = (release.assets || []).find((a) => a.name === "Pointer.dmg");
      const date = release.published_at ? new Date(release.published_at).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" }) : "";
      const size = dmg ? `${Math.round(dmg.size / 1e6)} MB` : "";
      const text = [`Version ${version}`, date, size].filter(Boolean).join(" · ");
      document.querySelectorAll("[data-version]").forEach((el) => (el.textContent = text));
    })
    .catch(() => {});
})();
