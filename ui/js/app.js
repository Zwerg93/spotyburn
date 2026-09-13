/**
 * SpotyBurn Dashboard & Real-Time Log Bridge
 * Tauri v2 Frontend Application
 */

const RED_BOOK_MAX_MS = 4800000; // 80 Minuten (4,800,000 ms)
const STANDARD_PLAY_74_MS = 4440000; // 74 Minuten (92.5%)
const DATA_CD_MAX_BYTES = 734003200; // 700 MB
const MP3_256K_BYTES_PER_MS = 32; // 256 kbps = 32 bytes/ms

// Tauri API Bridge detection
const tauri = window.__TAURI__;
const tauriInvoke = tauri?.core?.invoke || tauri?.invoke;
const tauriListen = tauri?.event?.listen;

// Application State
const state = {
  config: {
    client_id: "",
    client_secret: "",
    cache_dir: "",
    default_burn_mode: "AudioCdRedBook",
  },
  userProfile: null,
  playlists: [],
  tracks: [],
  selectedTrackIds: new Set(),
  drives: [],
  selectedDriveId: "",
  mediaStatus: null,
  burnMode: "AudioCdRedBook", // "AudioCdRedBook" | "DataMp3Cd" | "ExportOnly"
  isBurning: false,
  searchDebounceTimer: null,
  searchResults: { tracks: [], albums: [], playlists: [] },
  activeSearchTab: "all",
  unreadLogs: false,
  burnPollingInterval: null,
  lastLogIndex: 0,
};

// DOM Elements cache
const elements = {};

function initElements() {
  // Header Elements
  elements.spotifyLoginBtn = document.getElementById("spotify-login-btn");
  elements.userBadge = document.getElementById("user-badge");
  elements.userAvatar = document.getElementById("user-avatar");
  elements.userAvatarFallback = document.getElementById("user-avatar-fallback");
  elements.userName = document.getElementById("user-name");
  elements.spotifyLogoutBtn = document.getElementById("spotify-logout-btn");
  elements.openLogsBtn = document.getElementById("open-logs-btn");
  elements.openSettingsBtn = document.getElementById("open-settings-btn");
  elements.logStatusBadge = document.getElementById("log-status-badge");

  // Sidebar Elements
  elements.sidebarPlaylists = document.getElementById("sidebar-playlists");
  elements.refreshPlaylistsBtn = document.getElementById("refresh-playlists-btn");
  elements.playlistCountBadge = document.getElementById("playlist-count-badge");
  elements.playlistList = document.getElementById("playlist-list");
  elements.playlistLoginPrompt = document.getElementById("playlist-login-prompt");
  elements.sidebarLoginBtn = document.getElementById("sidebar-login-btn");

  // Ingestion: Search & URL
  elements.searchContainer = document.querySelector(".search-container");
  elements.spotifySearchInput = document.getElementById("spotify-search-input");
  elements.clearSearchBtn = document.getElementById("clear-search-btn");
  elements.searchSpinner = document.getElementById("search-spinner");
  elements.searchDropdown = document.getElementById("search-dropdown");
  elements.searchResultsList = document.getElementById("search-results-list");
  elements.searchTabs = document.querySelectorAll(".search-tab");
  elements.addAllSearchTracksBtn = document.getElementById("add-all-search-tracks-btn");

  elements.toggleUrlBtn = document.getElementById("toggle-url-btn");
  elements.urlToggleChevron = document.getElementById("url-toggle-chevron");
  elements.urlInputCollapse = document.getElementById("url-input-collapse");
  elements.spotifyUrlInput = document.getElementById("spotify-url-input");
  elements.fetchTracksBtn = document.getElementById("fetch-tracks-btn");
  elements.fetchStatusMsg = document.getElementById("fetch-status-msg");

  // Destination Folder Elements
  elements.currentDestFolder = document.getElementById("current-dest-folder");
  elements.chooseDestFolderBtn = document.getElementById("choose-dest-folder-btn");
  elements.openDestFolderBtn = document.getElementById("open-dest-folder-btn");

  // Capacity Meter Elements
  elements.capacityModeTitle = document.getElementById("capacity-mode-title");
  elements.capacitySubtitle = document.getElementById("capacity-subtitle");
  elements.capacityDurationText = document.getElementById("capacity-duration-text");
  elements.capacityLimitText = document.getElementById("capacity-limit-text");
  elements.capacityPctText = document.getElementById("capacity-pct-text");
  elements.capacityBar = document.getElementById("capacity-bar");
  elements.capacityMarker74 = document.getElementById("capacity-marker-74");
  elements.capacityWarningBanner = document.getElementById("capacity-warning-banner");

  // Tracklist & Table Elements
  elements.selectedTracksCount = document.getElementById("selected-tracks-count");
  elements.selectAllBtn = document.getElementById("select-all-btn");
  elements.selectNoneBtn = document.getElementById("select-none-btn");
  elements.clearTracksBtn = document.getElementById("clear-tracks-btn");
  elements.masterTrackCheckbox = document.getElementById("master-track-checkbox");
  elements.tracksTableBody = document.getElementById("tracks-tbody");

  // Bottom Control & Mode Switcher
  elements.modeTabs = document.querySelectorAll(".mode-tab");
  elements.driveControlsContainer = document.getElementById("drive-controls-container");
  elements.exportControlsContainer = document.getElementById("export-controls-container");
  elements.openExportDirBtn = document.getElementById("open-export-dir-btn");

  // Drive Controls
  elements.driveSelect = document.getElementById("drive-select");
  elements.refreshDrivesBtn = document.getElementById("refresh-drives-btn");
  elements.ejectDriveBtn = document.getElementById("eject-drive-btn");
  elements.burnSpeedSelect = document.getElementById("burn-speed-select");
  elements.mediaStatusDot = document.getElementById("media-status-dot");
  elements.mediaStatusText = document.getElementById("media-status-text");
  elements.checkMediaBtn = document.getElementById("check-media-btn");
  elements.ejectAfterCheckbox = document.getElementById("eject-after-checkbox");
  elements.simulateCheckbox = document.getElementById("simulate-checkbox");

  // Action Button
  elements.startBurnBtn = document.getElementById("start-burn-btn");
  elements.actionBtnIcon = document.getElementById("action-btn-icon");
  elements.actionBtnLabel = document.getElementById("action-btn-label");

  // Settings Modal
  elements.settingsModal = document.getElementById("settings-modal");
  elements.closeSettingsBtn = document.getElementById("close-settings-btn");
  elements.cancelSettingsBtn = document.getElementById("cancel-settings-btn");
  elements.clientIdInput = document.getElementById("client-id-input");
  elements.clientSecretInput = document.getElementById("client-secret-input");
  elements.toggleSecretBtn = document.getElementById("toggle-secret-btn");
  elements.cacheDirInput = document.getElementById("cache-dir-input");
  elements.modalChooseCacheBtn = document.getElementById("modal-choose-cache-btn");
  elements.modalOpenCacheBtn = document.getElementById("modal-open-cache-btn");
  elements.saveConfigBtn = document.getElementById("save-config-btn");
  elements.authStatusMsg = document.getElementById("auth-status-msg");

  // Logs Modal
  elements.logsModal = document.getElementById("logs-modal");
  elements.closeLogsBtn = document.getElementById("close-logs-btn");
  elements.closeLogsFooterBtn = document.getElementById("close-logs-footer-btn");
  elements.burnStageText = document.getElementById("burn-stage-text");
  elements.burnPctText = document.getElementById("burn-pct-text");
  elements.burnProgressFill = document.getElementById("burn-progress-fill");
  elements.burnDetailText = document.getElementById("burn-detail-text");
  elements.consoleWindow = document.getElementById("console-window");
  elements.clearLogsBtn = document.getElementById("clear-logs-btn");
  elements.autoscrollCheckbox = document.getElementById("autoscroll-checkbox");
}

// Helpers
function formatDurationMs(ms) {
  if (!ms || ms <= 0) return "00:00";
  const totalSecs = Math.floor(ms / 1000);
  const hours = Math.floor(totalSecs / 3600);
  const mins = Math.floor((totalSecs % 3600) / 60);
  const secs = totalSecs % 60;
  if (hours > 0) {
    return `${String(hours).padStart(2, "0")}:${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  }
  return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
}

function formatBytesMb(bytes) {
  if (!bytes || bytes <= 0) return "0.0 MB";
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(1)} MB`;
}

function getTimestamp() {
  const now = new Date();
  return now.toTimeString().split(" ")[0];
}

function logMessage(level, message) {
  if (!elements.consoleWindow) return;
  const line = document.createElement("div");
  line.className = `log-line ${level}`;

  const timeSpan = document.createElement("span");
  timeSpan.className = "log-time";
  timeSpan.textContent = `[${getTimestamp()}]`;

  const levelSpan = document.createElement("span");
  levelSpan.className = `log-level ${level}`;
  levelSpan.textContent = level.toUpperCase();

  const msgSpan = document.createElement("span");
  msgSpan.className = "log-msg";
  msgSpan.textContent = message;

  line.appendChild(timeSpan);
  line.appendChild(levelSpan);
  line.appendChild(msgSpan);

  elements.consoleWindow.appendChild(line);

  if (elements.autoscrollCheckbox?.checked) {
    elements.consoleWindow.scrollTop = elements.consoleWindow.scrollHeight;
  }

  // Update badge if modal is closed and a warning/error/info occurred
  if (elements.logsModal?.style.display === "none") {
    if (level === "error") {
      elements.logStatusBadge?.classList.add("error");
    } else if (state.isBurning) {
      elements.logStatusBadge?.classList.add("active");
    }
  }
}

function escapeHtml(str) {
  if (!str) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

// IPC Wrappers
async function callIpc(cmd, args = {}) {
  if (typeof tauriInvoke === "function") {
    return await tauriInvoke(cmd, args);
  }
  console.warn(`Tauri IPC not available. Mocking command '${cmd}'`, args);
  return mockIpc(cmd, args);
}

function mockIpc(cmd, args) {
  switch (cmd) {
    case "get_config":
      return Promise.resolve({
        client_id: "",
        client_secret: "",
        cache_dir: "~/.spotyburn/cache",
        default_burn_mode: "AudioCdRedBook",
      });
    case "save_config":
      return Promise.resolve();
    case "get_user_profile":
      return Promise.resolve({
        id: "mock_user",
        display_name: "Demo Audiophile",
        image_url: null,
      });
    case "spotify_login":
      return Promise.resolve({
        id: "mock_user",
        display_name: "Demo Audiophile",
        image_url: null,
      });
    case "spotify_logout":
      return Promise.resolve();
    case "get_user_playlists":
      return Promise.resolve([
        {
          id: "37i9dQZF1DXcBWIGoYBM5M",
          name: "Today's Top Hits",
          description: "Top hits of today",
          track_count: 50,
          image_url: null,
        },
        {
          id: "37i9dQZF1DX4WYpdgoIcn6",
          name: "Chill Tracks 2026",
          description: "Relaxing deep focus audio",
          track_count: 24,
          image_url: null,
        },
        {
          id: "37i9dQZF1DX10zKzsJ2jva",
          name: "Viva Latino",
          description: "Latin rhythmic beats",
          track_count: 35,
          image_url: null,
        },
      ]);
    case "search_spotify": {
      const q = (args.query || "").toLowerCase();
      return Promise.resolve({
        tracks: [
          {
            id: "s1",
            title: `Track for "${q}"`,
            artists: ["Daft Punk"],
            album: "Random Access Memories",
            duration_ms: 275000,
            track_number: 1,
            isrc: null,
          },
          {
            id: "s2",
            title: "Get Lucky",
            artists: ["Daft Punk", "Pharrell Williams"],
            album: "Random Access Memories",
            duration_ms: 248000,
            track_number: 8,
            isrc: null,
          },
        ],
        albums: [
          {
            id: "alb1",
            name: "Discovery",
            artists: ["Daft Punk"],
            total_tracks: 14,
            release_date: "2001-03-12",
            image_url: null,
          },
        ],
        playlists: [
          {
            id: "pl1",
            name: `Best of ${args.query || "Music"}`,
            description: "Curated collection",
            track_count: 18,
            image_url: null,
          },
        ],
      });
    }
    case "get_optical_drives":
      return Promise.resolve([
        { id: "0", vendor: "HL-DT-ST", product: "DVDRAM GP60NB60", interconnect: "USB" },
      ]);
    case "get_media_status":
      return Promise.resolve({
        drive_id: args.driveId || "0",
        media_present: true,
        is_blank: true,
        media_type: "CD-R",
        free_blocks: 360000,
        free_minutes: 80.0,
      });
    case "eject_drive":
      return Promise.resolve();
    case "open_cache_folder":
      return Promise.resolve("~/.spotyburn/cache");
    case "calculate_capacity": {
      const tracks = args.tracks || [];
      const mode = args.mode || "AudioCdRedBook";
      const totalMs = tracks.reduce((acc, t) => acc + (t.duration_ms || 0), 0);
      if (mode === "AudioCdRedBook") {
        return Promise.resolve({
          used_ms: totalMs,
          max_ms: RED_BOOK_MAX_MS,
          used_bytes: (totalMs * 176400) / 1000,
          max_bytes: (RED_BOOK_MAX_MS * 176400) / 1000,
          used_percent: (totalMs / RED_BOOK_MAX_MS) * 100,
          is_exceeded: totalMs > RED_BOOK_MAX_MS,
          mode_label: "Audio CD (Red Book)",
        });
      } else if (mode === "DataMp3Cd") {
        const usedBytes = totalMs * MP3_256K_BYTES_PER_MS;
        return Promise.resolve({
          used_ms: totalMs,
          max_ms: DATA_CD_MAX_BYTES / MP3_256K_BYTES_PER_MS,
          used_bytes: usedBytes,
          max_bytes: DATA_CD_MAX_BYTES,
          used_percent: (usedBytes / DATA_CD_MAX_BYTES) * 100,
          is_exceeded: usedBytes > DATA_CD_MAX_BYTES,
          mode_label: "Data / MP3 CD",
        });
      } else {
        return Promise.resolve({
          used_ms: totalMs,
          max_ms: 0,
          used_bytes: (totalMs * 176400) / 1000,
          max_bytes: 0,
          used_percent: 0,
          is_exceeded: false,
          mode_label: "Virtueller Test-Export (Ohne Brenner)",
        });
      }
    }
    case "fetch_spotify_tracks":
      return Promise.resolve({
        tracks: [
          { id: "t1", title: "Time", artists: ["Pink Floyd"], album: "The Dark Side of the Moon", duration_ms: 413000, track_number: 1, isrc: null },
          { id: "t2", title: "Money", artists: ["Pink Floyd"], album: "The Dark Side of the Moon", duration_ms: 382000, track_number: 2, isrc: null },
          { id: "t3", title: "Us and Them", artists: ["Pink Floyd"], album: "The Dark Side of the Moon", duration_ms: 469000, track_number: 3, isrc: null },
        ],
        total_duration_ms: 1264000,
        total_duration_formatted: "21:04",
        track_count: 3,
      });
    case "start_burn_job":
      return Promise.resolve("Burn job started");
    default:
      return Promise.reject(new Error(`Unknown mock command: ${cmd}`));
  }
}

// ==========================================================================
// User Authentication & Spotify Profile
// ==========================================================================
async function checkUserProfile() {
  try {
    const profile = await callIpc("get_user_profile");
    if (profile) {
      setUserProfile(profile);
      await loadUserPlaylists();
    } else {
      setUserProfile(null);
    }
  } catch (err) {
    console.warn("Could not check user profile:", err);
    setUserProfile(null);
  }
}

function setUserProfile(profile) {
  state.userProfile = profile;
  if (profile) {
    elements.spotifyLoginBtn.style.display = "none";
    elements.userBadge.style.display = "flex";
    elements.userName.textContent = profile.display_name || "Spotify User";

    if (profile.image_url) {
      elements.userAvatar.src = profile.image_url;
      elements.userAvatar.style.display = "block";
      elements.userAvatarFallback.style.display = "none";
    } else {
      elements.userAvatar.style.display = "none";
      elements.userAvatarFallback.style.display = "block";
    }

    elements.playlistLoginPrompt.style.display = "none";
  } else {
    elements.spotifyLoginBtn.style.display = "inline-flex";
    elements.userBadge.style.display = "none";
    elements.playlistList.innerHTML = "";
    elements.playlistLoginPrompt.style.display = "block";
    elements.playlistCountBadge.textContent = "0";
  }
}

async function loginSpotify() {
  try {
    logMessage("info", "Starte Spotify OAuth2 Loopback-Login...");
    elements.spotifyLoginBtn.disabled = true;
    elements.sidebarLoginBtn.disabled = true;

    const profile = await callIpc("spotify_login");
    setUserProfile(profile);
    logMessage("success", `✓ Erfolgreich angemeldet als: ${profile.display_name}`);
    await loadUserPlaylists();
  } catch (err) {
    logMessage("error", `Spotify Login fehlgeschlagen: ${err}`);
    alert(`Spotify Login fehlgeschlagen: ${err}`);
  } finally {
    elements.spotifyLoginBtn.disabled = false;
    elements.sidebarLoginBtn.disabled = false;
  }
}

async function logoutSpotify() {
  try {
    await callIpc("spotify_logout");
    setUserProfile(null);
    state.playlists = [];
    logMessage("info", "Erfolgreich von Spotify abgemeldet.");
  } catch (err) {
    logMessage("error", `Abmelden fehlgeschlagen: ${err}`);
  }
}

// ==========================================================================
// Sidebar Playlists Management
// ==========================================================================
async function loadUserPlaylists() {
  if (!state.userProfile) return;

  try {
    elements.refreshPlaylistsBtn.disabled = true;
    elements.refreshPlaylistsBtn.textContent = "⏳";
    const playlists = await callIpc("get_user_playlists");
    state.playlists = playlists || [];
    renderPlaylists();
    logMessage("info", `${state.playlists.length} persönliche Playlists geladen.`);
  } catch (err) {
    logMessage("warn", `Playlists konnten nicht geladen werden: ${err}`);
  } finally {
    elements.refreshPlaylistsBtn.disabled = false;
    elements.refreshPlaylistsBtn.textContent = "🔄";
  }
}

function renderPlaylists() {
  const container = elements.playlistList;
  container.innerHTML = "";
  elements.playlistCountBadge.textContent = String(state.playlists.length);

  if (state.playlists.length === 0) {
    elements.playlistLoginPrompt.style.display = "block";
    elements.playlistLoginPrompt.querySelector("h4").textContent = "Keine Playlists gefunden";
    elements.playlistLoginPrompt.querySelector("p").textContent = "Du hast noch keine Playlists in deinem Account erstellt.";
    elements.sidebarLoginBtn.style.display = "none";
    return;
  }

  elements.playlistLoginPrompt.style.display = "none";

  state.playlists.forEach((pl) => {
    const card = document.createElement("div");
    card.className = "playlist-card";
    card.dataset.playlistId = pl.id;

    const thumbHtml = pl.image_url
      ? `<img src="${escapeHtml(pl.image_url)}" class="playlist-thumb" alt="${escapeHtml(pl.name)}" />`
      : `<span class="playlist-thumb-fallback">🎵</span>`;

    card.innerHTML = `
      <div class="playlist-thumb-wrap">${thumbHtml}</div>
      <div class="playlist-meta">
        <div class="playlist-title" title="${escapeHtml(pl.name)}">${escapeHtml(pl.name)}</div>
        <div class="playlist-track-count">${pl.track_count || 0} Tracks</div>
      </div>
    `;

    card.addEventListener("click", () => {
      // Highlight active card
      document.querySelectorAll(".playlist-card").forEach((c) => c.classList.remove("active"));
      card.classList.add("active");
      // Directly fetch playlist tracks
      fetchTracksFromUri(`spotify:playlist:${pl.id}`, pl.name);
    });

    container.appendChild(card);
  });
}

// ==========================================================================
// In-App Spotify Search & Dropdown Preview
// ==========================================================================
function setupSearchHandlers() {
  const input = elements.spotifySearchInput;
  const clearBtn = elements.clearSearchBtn;

  input.addEventListener("input", (e) => {
    const val = e.target.value;
    clearBtn.style.display = val.length > 0 ? "block" : "none";

    if (state.searchDebounceTimer) clearTimeout(state.searchDebounceTimer);

    if (val.trim().length === 0) {
      elements.searchDropdown.style.display = "none";
      elements.searchSpinner.style.display = "none";
      return;
    }

    elements.searchSpinner.style.display = "block";
    state.searchDebounceTimer = setTimeout(() => {
      performSearch(val.trim());
    }, 300);
  });

  clearBtn.addEventListener("click", () => {
    input.value = "";
    clearBtn.style.display = "none";
    elements.searchDropdown.style.display = "none";
    input.focus();
  });

  // Search Tabs
  elements.searchTabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      elements.searchTabs.forEach((t) => t.classList.remove("active"));
      tab.classList.add("active");
      state.activeSearchTab = tab.dataset.tab;
      renderSearchResults();
    });
  });

  // Bulk add all tracks from current search results
  elements.addAllSearchTracksBtn?.addEventListener("click", (e) => {
    e.stopPropagation();
    const tracksToAdd = (state.searchResults.tracks || []).filter(
      (t) => !state.tracks.some((st) => st.id === t.id)
    );
    if (tracksToAdd.length === 0) return;

    tracksToAdd.forEach((t) => {
      state.tracks.push(t);
      state.selectedTrackIds.add(t.id);
    });

    renderTracksTable();
    updateCapacityMeter();
    renderSearchResults();
    logMessage("success", `✓ ${tracksToAdd.length} Tracks zur Brennliste hinzugefügt.`);
  });

  // Close dropdown on click outside
  document.addEventListener("click", (e) => {
    if (!elements.searchContainer?.contains(e.target) && !elements.searchDropdown?.contains(e.target)) {
      elements.searchDropdown.style.display = "none";
    }
  });

  // Close on Escape
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      elements.searchDropdown.style.display = "none";
    }
  });
}

async function performSearch(query) {
  try {
    const result = await callIpc("search_spotify", {
      query,
      searchType: "all",
      limit: 10,
    });
    state.searchResults = result || { tracks: [], albums: [], playlists: [] };
    renderSearchResults();
    elements.searchDropdown.style.display = "flex";
  } catch (err) {
    logMessage("error", `Suchfehler: ${err}`);
  } finally {
    elements.searchSpinner.style.display = "none";
  }
}

function renderSearchResults() {
  const container = elements.searchResultsList;
  container.innerHTML = "";

  const tab = state.activeSearchTab;
  const tracks = tab === "all" || tab === "tracks" ? state.searchResults.tracks || [] : [];
  const albums = tab === "all" || tab === "albums" ? state.searchResults.albums || [] : [];
  const playlists = tab === "all" || tab === "playlists" ? state.searchResults.playlists || [] : [];

  const totalResults = tracks.length + albums.length + playlists.length;
  if (totalResults === 0) {
    if (elements.addAllSearchTracksBtn) elements.addAllSearchTracksBtn.style.display = "none";
    container.innerHTML = `<div class="search-empty">Keine Ergebnisse für diese Suche gefunden.</div>`;
    return;
  }

  // Update "+ Alle Tracks hinzufügen" button in search tabs
  if (elements.addAllSearchTracksBtn) {
    if (tracks.length > 0 && (tab === "all" || tab === "tracks")) {
      elements.addAllSearchTracksBtn.style.display = "inline-flex";
      const unaddedTracks = tracks.filter((t) => !state.tracks.some((st) => st.id === t.id));
      if (unaddedTracks.length === 0) {
        elements.addAllSearchTracksBtn.textContent = "✓ Alle hinzugefügt";
        elements.addAllSearchTracksBtn.disabled = true;
        elements.addAllSearchTracksBtn.classList.add("btn-added");
        elements.addAllSearchTracksBtn.classList.remove("btn-primary");
      } else {
        elements.addAllSearchTracksBtn.textContent = `+ Alle Tracks (${unaddedTracks.length})`;
        elements.addAllSearchTracksBtn.disabled = false;
        elements.addAllSearchTracksBtn.classList.remove("btn-added");
        elements.addAllSearchTracksBtn.classList.add("btn-primary");
      }
    } else {
      elements.addAllSearchTracksBtn.style.display = "none";
    }
  }

  // Render Tracks
  tracks.forEach((track) => {
    const item = document.createElement("div");
    item.className = "search-item";
    const artists = track.artists?.join(", ") || "Unknown Artist";
    const duration = formatDurationMs(track.duration_ms);
    const isAdded = state.tracks.some((t) => t.id === track.id);

    item.innerHTML = `
      <div class="search-item-left">
        <div class="search-item-icon">🎵</div>
        <div class="search-item-info">
          <div class="search-item-title" title="${escapeHtml(track.title)}">${escapeHtml(track.title)}</div>
          <div class="search-item-subtitle">${escapeHtml(artists)} • ${escapeHtml(track.album || "")} (${duration})</div>
        </div>
      </div>
      <div class="search-item-btn">
        <button type="button" class="btn btn-sm ${isAdded ? "btn-added" : "btn-primary"}" ${isAdded ? "disabled" : ""}>
          ${isAdded ? "✓ Hinzugefügt" : "+ Hinzufügen"}
        </button>
      </div>
    `;

    const btn = item.querySelector("button");
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      if (state.tracks.some((t) => t.id === track.id)) return;
      addSingleTrack(track);
      btn.textContent = "✓ Hinzugefügt";
      btn.classList.remove("btn-primary");
      btn.classList.add("btn-added");
      btn.disabled = true;

      // Also update "Alle hinzufügen" button
      if (elements.addAllSearchTracksBtn && (state.activeSearchTab === "all" || state.activeSearchTab === "tracks")) {
        const remaining = (state.searchResults.tracks || []).filter((t) => !state.tracks.some((st) => st.id === t.id)).length;
        if (remaining === 0) {
          elements.addAllSearchTracksBtn.textContent = "✓ Alle hinzugefügt";
          elements.addAllSearchTracksBtn.disabled = true;
          elements.addAllSearchTracksBtn.classList.add("btn-added");
          elements.addAllSearchTracksBtn.classList.remove("btn-primary");
        } else {
          elements.addAllSearchTracksBtn.textContent = `+ Alle Tracks (${remaining})`;
        }
      }
    });

    container.appendChild(item);
  });

  // Render Albums
  albums.forEach((album) => {
    const item = document.createElement("div");
    item.className = "search-item";
    const artists = album.artists?.join(", ") || "";
    const thumbHtml = album.image_url
      ? `<img src="${escapeHtml(album.image_url)}" class="search-item-thumb" alt="${escapeHtml(album.name)}" />`
      : `<div class="search-item-icon">💿</div>`;

    item.innerHTML = `
      <div class="search-item-left">
        ${thumbHtml}
        <div class="search-item-info">
          <div class="search-item-title" title="${escapeHtml(album.name)}">${escapeHtml(album.name)}</div>
          <div class="search-item-subtitle">Album von ${escapeHtml(artists)} • ${album.total_tracks || 0} Tracks</div>
        </div>
      </div>
      <div class="search-item-btn">
        <button type="button" class="btn btn-primary btn-sm">Laden</button>
      </div>
    `;

    item.querySelector("button").addEventListener("click", (e) => {
      e.stopPropagation();
      elements.searchDropdown.style.display = "none";
      fetchTracksFromUri(`spotify:album:${album.id}`, album.name);
    });

    container.appendChild(item);
  });

  // Render Playlists
  playlists.forEach((pl) => {
    const item = document.createElement("div");
    item.className = "search-item";
    const thumbHtml = pl.image_url
      ? `<img src="${escapeHtml(pl.image_url)}" class="search-item-thumb" alt="${escapeHtml(pl.name)}" />`
      : `<div class="search-item-icon">📂</div>`;

    item.innerHTML = `
      <div class="search-item-left">
        ${thumbHtml}
        <div class="search-item-info">
          <div class="search-item-title" title="${escapeHtml(pl.name)}">${escapeHtml(pl.name)}</div>
          <div class="search-item-subtitle">Playlist • ${pl.track_count || 0} Tracks</div>
        </div>
      </div>
      <div class="search-item-btn">
        <button type="button" class="btn btn-primary btn-sm">Laden</button>
      </div>
    `;

    item.querySelector("button").addEventListener("click", (e) => {
      e.stopPropagation();
      elements.searchDropdown.style.display = "none";
      fetchTracksFromUri(`spotify:playlist:${pl.id}`, pl.name);
    });

    container.appendChild(item);
  });
}

function addSingleTrack(track) {
  // Prevent exact duplicate IDs if already present
  const exists = state.tracks.some((t) => t.id === track.id);
  if (exists) {
    logMessage("warn", `Track '${track.title}' befindet sich bereits in der Brennliste.`);
    return;
  }

  state.tracks.push(track);
  state.selectedTrackIds.add(track.id);

  renderTracksTable();
  updateCapacityMeter();

  logMessage("success", `✓ Track '${track.title}' zur Brennliste hinzugefügt.`);
}

// ==========================================================================
// Direct URL Ingestion & Track Loading
// ==========================================================================
async function fetchTracksFromUri(uri, name = "") {
  try {
    elements.fetchStatusMsg.textContent = `Lade "${name || uri}"...`;
    elements.fetchStatusMsg.className = "status-msg info";
    logMessage("info", `Rufe Tracks ab: ${name || uri}`);

    const result = await callIpc("fetch_spotify_tracks", { url: uri });
    state.tracks = result.tracks || [];
    state.selectedTrackIds = new Set(state.tracks.map((t) => t.id));

    renderTracksTable();
    updateCapacityMeter();

    elements.fetchStatusMsg.textContent = `✓ ${result.track_count} Tracks geladen (${result.total_duration_formatted})`;
    elements.fetchStatusMsg.className = "status-msg success";
    logMessage("success", `${result.track_count} Tracks von "${name || uri}" erfolgreich geladen.`);
  } catch (err) {
    elements.fetchStatusMsg.textContent = `Fehler: ${err}`;
    elements.fetchStatusMsg.className = "status-msg error";
    logMessage("error", `Ingestion fehlgeschlagen: ${err}`);
  }
}

async function handleManualUrlFetch() {
  const url = elements.spotifyUrlInput.value.trim();
  if (!url) {
    elements.fetchStatusMsg.textContent = "Bitte eine Spotify Playlist-, Album- oder Track-URL eingeben.";
    elements.fetchStatusMsg.className = "status-msg error";
    return;
  }
  elements.fetchTracksBtn.disabled = true;
  await fetchTracksFromUri(url);
  elements.fetchTracksBtn.disabled = false;
}

// ==========================================================================
// Track Table & Selection Management
// ==========================================================================
function renderTracksTable() {
  const tbody = elements.tracksTableBody;
  tbody.innerHTML = "";

  if (state.tracks.length === 0) {
    const tr = document.createElement("tr");
    tr.innerHTML = `<td colspan="7" class="empty-state">Noch keine Tracks geladen. Wähle eine Playlist links aus, suche oben nach Songs oder gib eine Spotify-URL ein.</td>`;
    tbody.appendChild(tr);
    updateMasterCheckboxState();
    return;
  }

  state.tracks.forEach((track, index) => {
    const tr = document.createElement("tr");

    const isChecked = state.selectedTrackIds.has(track.id);
    const trackNum = track.track_number || index + 1;
    const artists = track.artists?.join(", ") || "Unknown";
    const duration = formatDurationMs(track.duration_ms);

    tr.innerHTML = `
      <td class="col-check"><input type="checkbox" data-track-id="${track.id}" ${isChecked ? "checked" : ""} /></td>
      <td class="col-num">${trackNum}</td>
      <td class="col-title" title="${escapeHtml(track.title)}">${escapeHtml(track.title)}</td>
      <td class="col-artist" title="${escapeHtml(artists)}">${escapeHtml(artists)}</td>
      <td class="col-album" title="${escapeHtml(track.album || "")}">${escapeHtml(track.album || "")}</td>
      <td class="col-duration">${duration}</td>
      <td class="col-action"><button type="button" class="btn-remove-track" title="Entfernen">✕</button></td>
    `;

    // Checkbox change
    const checkbox = tr.querySelector('input[type="checkbox"]');
    checkbox.addEventListener("change", (e) => {
      if (e.target.checked) {
        state.selectedTrackIds.add(track.id);
      } else {
        state.selectedTrackIds.delete(track.id);
      }
      updateMasterCheckboxState();
      updateCapacityMeter();
    });

    // Remove single track
    const removeBtn = tr.querySelector(".btn-remove-track");
    removeBtn.addEventListener("click", () => {
      state.tracks = state.tracks.filter((t) => t.id !== track.id);
      state.selectedTrackIds.delete(track.id);
      renderTracksTable();
      updateCapacityMeter();
    });

    tbody.appendChild(tr);
  });

  updateMasterCheckboxState();
}

function updateMasterCheckboxState() {
  if (!elements.masterTrackCheckbox) return;
  const total = state.tracks.length;
  const selected = state.selectedTrackIds.size;

  if (total === 0) {
    elements.masterTrackCheckbox.checked = false;
    elements.masterTrackCheckbox.indeterminate = false;
  } else if (selected === total) {
    elements.masterTrackCheckbox.checked = true;
    elements.masterTrackCheckbox.indeterminate = false;
  } else if (selected === 0) {
    elements.masterTrackCheckbox.checked = false;
    elements.masterTrackCheckbox.indeterminate = false;
  } else {
    elements.masterTrackCheckbox.checked = false;
    elements.masterTrackCheckbox.indeterminate = true;
  }
}

function selectAllTracks(select) {
  if (select) {
    state.tracks.forEach((t) => state.selectedTrackIds.add(t.id));
  } else {
    state.selectedTrackIds.clear();
  }

  const checkboxes = elements.tracksTableBody.querySelectorAll('input[type="checkbox"]');
  checkboxes.forEach((cb) => {
    cb.checked = select;
  });

  updateMasterCheckboxState();
  updateCapacityMeter();
}

function clearAllTracks() {
  state.tracks = [];
  state.selectedTrackIds.clear();
  renderTracksTable();
  updateCapacityMeter();
  logMessage("info", "Trackliste geleert.");
}

// ==========================================================================
// REAKTIVER KAPAZITÄTSBALKEN (Sofortige Umschaltung bei Moduswechsel)
// ==========================================================================
async function updateCapacityMeter() {
  const selectedTracks = state.tracks.filter((t) => state.selectedTrackIds.has(t.id));
  const selectedCount = selectedTracks.length;
  const totalMs = selectedTracks.reduce((acc, t) => acc + (t.duration_ms || 0), 0);

  // Update counts label
  elements.selectedTracksCount.textContent = `${selectedCount} von ${state.tracks.length} Tracks ausgewählt`;

  const bar = elements.capacityBar;
  bar.classList.remove("warning", "exceeded", "mode-data", "mode-export");
  elements.capacityWarningBanner.classList.remove("visible");

  // Call Tauri calculate_capacity command (or mock)
  let usage = null;
  try {
    usage = await callIpc("calculate_capacity", {
      tracks: selectedTracks,
      mode: state.burnMode,
    });
  } catch (err) {
    console.warn("Error calling calculate_capacity IPC:", err);
  }

  const mode = state.burnMode;

  if (mode === "AudioCdRedBook") {
    // MODUS 1: Red Book Audio CD (80 Min Limit)
    elements.capacityModeTitle.textContent = "Audio CD (Red Book 80 Min)";
    elements.capacitySubtitle.textContent = "Max. 80 Minuten Audio nach CD-DA Standard";
    elements.capacityMarker74.style.display = "block";

    const formattedDuration = formatDurationMs(totalMs);
    elements.capacityDurationText.textContent = formattedDuration;

    const pct = (totalMs / RED_BOOK_MAX_MS) * 100;
    elements.capacityLimitText.innerHTML = ` / 80:00 (<span id="capacity-pct-text">${pct.toFixed(1)}%</span>)`;

    bar.style.width = `${Math.min(pct, 100)}%`;

    if (totalMs > RED_BOOK_MAX_MS) {
      // > 80 Min: Rot mit animated Hazard-Stripes
      bar.classList.add("exceeded");
      elements.capacityWarningBanner.textContent = `⚠️ Kapazität überschritten! Die gewählten Tracks (${formattedDuration}) überschreiten das 80-Minuten-Limit des Red Book CD-DA Standards. Bitte Tracks abwählen oder in den Modus Data/MP3 CD wechseln.`;
      elements.capacityWarningBanner.classList.add("visible");
      elements.startBurnBtn.disabled = true;
      elements.startBurnBtn.title = "Audio CD Kapazität von 80 Minuten überschritten";
    } else if (totalMs > STANDARD_PLAY_74_MS) {
      // 74 - 80 Min: Gelb/Amber
      bar.classList.add("warning");
      elements.startBurnBtn.disabled = selectedCount === 0 || state.isBurning;
      elements.startBurnBtn.title = "";
    } else {
      // < 74 Min: Grün/Emerald
      elements.startBurnBtn.disabled = selectedCount === 0 || state.isBurning;
      elements.startBurnBtn.title = "";
    }
  } else if (mode === "DataMp3Cd") {
    // MODUS 2: Data / MP3 CD (700 MB Limit)
    elements.capacityModeTitle.textContent = "Data / MP3 CD (700 MB)";
    elements.capacitySubtitle.textContent = "MP3 Transkodierung (~120-160 Songs Kapazität, kein 80-Minuten-Limit)";
    elements.capacityMarker74.style.display = "none";

    const totalBytes = totalMs * MP3_256K_BYTES_PER_MS;
    const formattedMb = formatBytesMb(totalBytes);
    elements.capacityDurationText.textContent = formattedMb;

    const pct = (totalBytes / DATA_CD_MAX_BYTES) * 100;
    elements.capacityLimitText.innerHTML = ` / 700.0 MB (<span id="capacity-pct-text">${pct.toFixed(1)}%</span> • ~${selectedCount} Songs)`;

    bar.classList.add("mode-data");
    bar.style.width = `${Math.min(pct, 100)}%`;

    if (totalBytes > DATA_CD_MAX_BYTES) {
      // > 700 MB
      bar.classList.add("exceeded");
      elements.capacityWarningBanner.textContent = `⚠️ Kapazität überschritten! Das Datenvolumen (${formattedMb}) überschreitet das 700 MB Limit einer Standard Data-CD.`;
      elements.capacityWarningBanner.classList.add("visible");
      elements.startBurnBtn.disabled = true;
      elements.startBurnBtn.title = "Data CD Kapazität von 700 MB überschritten";
    } else {
      elements.startBurnBtn.disabled = selectedCount === 0 || state.isBurning;
      elements.startBurnBtn.title = "";
    }
  } else if (mode === "ExportOnly") {
    // MODUS 3: Virtueller Test / Export (Kein CD-Limit)
    elements.capacityModeTitle.textContent = "Virtueller Test / Export (Kein CD-Limit)";
    elements.capacitySubtitle.textContent = "Herunterladen, Transkodieren zu 44.1kHz 16-Bit WAV & CUE-Sheet Export";
    elements.capacityMarker74.style.display = "none";

    const formattedDuration = formatDurationMs(totalMs);
    const estWavBytes = (totalMs * 176400) / 1000;
    elements.capacityDurationText.textContent = formattedDuration;
    elements.capacityLimitText.innerHTML = ` • ~${formatBytesMb(estWavBytes)} (Virtueller Export)`;

    bar.classList.add("mode-export");
    bar.style.width = selectedCount > 0 ? "100%" : "0%";

    // Export mode has no capacity constraints!
    elements.startBurnBtn.disabled = selectedCount === 0 || state.isBurning;
    elements.startBurnBtn.title = "";
  }
}

function setBurnMode(mode) {
  state.burnMode = mode;

  // Update mode tabs UI
  elements.modeTabs.forEach((tab) => {
    if (tab.dataset.mode === mode) {
      tab.classList.add("active");
    } else {
      tab.classList.remove("active");
    }
  });

  // Switch drive controls vs export info box
  if (mode === "ExportOnly") {
    elements.driveControlsContainer.style.display = "none";
    elements.exportControlsContainer.style.display = "block";
    elements.actionBtnIcon.textContent = "📦";
    elements.actionBtnLabel.textContent = "Download & CUE exportieren";
  } else {
    elements.driveControlsContainer.style.display = "flex";
    elements.exportControlsContainer.style.display = "none";
    elements.actionBtnIcon.textContent = "🔥";
    elements.actionBtnLabel.textContent = "Download & Brennen";
  }

  // Sofortige reaktive Aktualisierung des Kapazitätsbalkens!
  updateCapacityMeter();
  logMessage("info", `Brennmodus gewechselt: ${mode}`);
}

// ==========================================================================
// Optical Drives & Media Status
// ==========================================================================
async function loadOpticalDrives() {
  elements.driveSelect.innerHTML = `<option value="">Laufwerke werden gesucht...</option>`;
  elements.refreshDrivesBtn.disabled = true;

  try {
    const drives = await callIpc("get_optical_drives");
    state.drives = drives || [];
    elements.driveSelect.innerHTML = "";

    if (state.drives.length === 0) {
      const opt = document.createElement("option");
      opt.value = "sim-0";
      opt.textContent = "Kein physikalischer Brenner erkannt (Simulationsmodus)";
      elements.driveSelect.appendChild(opt);
      state.selectedDriveId = "sim-0";
      elements.simulateCheckbox.checked = true;
      logMessage("warn", "Keine optischen Brenner erkannt. Simulationsmodus aktiviert.");
      updateMediaDisplay(null);
    } else {
      state.drives.forEach((d) => {
        const opt = document.createElement("option");
        opt.value = d.id;
        opt.textContent = `${d.vendor} ${d.product} (${d.interconnect}) [ID: ${d.id}]`;
        elements.driveSelect.appendChild(opt);
      });
      state.selectedDriveId = state.drives[0].id;
      logMessage("info", `${state.drives.length} optische(s) Laufwerk(e) gefunden.`);
      await checkMediaStatus();
    }
  } catch (err) {
    elements.driveSelect.innerHTML = `<option value="">Fehler bei Laufwerkserkennung</option>`;
    logMessage("error", `Laufwerkserkennung fehlgeschlagen: ${err}`);
  } finally {
    elements.refreshDrivesBtn.disabled = false;
  }
}

async function checkMediaStatus() {
  const driveId = elements.driveSelect.value || state.selectedDriveId;
  if (!driveId || driveId.startsWith("sim-")) {
    updateMediaDisplay({
      media_present: true,
      is_blank: true,
      media_type: "Virtual Disc",
      free_minutes: 80.0,
    });
    return;
  }

  elements.checkMediaBtn.disabled = true;
  elements.mediaStatusText.textContent = "Prüfe Medium...";

  try {
    const status = await callIpc("get_media_status", { driveId });
    state.mediaStatus = status;
    updateMediaDisplay(status);
  } catch (err) {
    updateMediaDisplay(null);
    logMessage("warn", `Medium-Status konnte nicht ermittelt werden: ${err}`);
  } finally {
    elements.checkMediaBtn.disabled = false;
  }
}

function updateMediaDisplay(status) {
  const dot = elements.mediaStatusDot;
  const text = elements.mediaStatusText;

  dot.className = "status-dot";

  if (!status || !status.media_present) {
    dot.classList.add("active-red");
    text.textContent = "Keine Disc im Laufwerk";
  } else if (status.is_blank) {
    dot.classList.add("active-green");
    const minStr = status.free_minutes ? `${status.free_minutes.toFixed(1)} Min frei` : "Beschreibbar";
    text.textContent = `Leer-CD erkannt (${status.media_type || "CD-R"} • ${minStr})`;
  } else {
    dot.classList.add("active-amber");
    text.textContent = `Disc bespielt / nicht leer (${status.media_type || "Unbekannt"})`;
  }
}

async function ejectDrive() {
  const driveId = elements.driveSelect.value || state.selectedDriveId;
  if (!driveId || driveId.startsWith("sim-")) {
    logMessage("info", "Simulierter Auswurf.");
    return;
  }

  try {
    elements.ejectDriveBtn.disabled = true;
    logMessage("info", `Werfe Medium aus Laufwerk '${driveId}' aus...`);
    await callIpc("eject_drive", { driveId });
    logMessage("success", "Medium erfolgreich ausgeworfen.");
    updateMediaDisplay({ media_present: false });
  } catch (err) {
    logMessage("error", `Auswerfen fehlgeschlagen: ${err}`);
  } finally {
    elements.ejectDriveBtn.disabled = false;
  }
}

// ==========================================================================
// Pipeline & Burn Job Execution
// ==========================================================================
async function startBurnJob() {
  const selectedTracks = state.tracks.filter((t) => state.selectedTrackIds.has(t.id));
  if (selectedTracks.length === 0) {
    alert("Bitte wähle mindestens einen Track aus.");
    return;
  }

  const isAudio = state.burnMode === "AudioCdRedBook";
  const isData = state.burnMode === "DataMp3Cd";
  const totalMs = selectedTracks.reduce((acc, t) => acc + (t.duration_ms || 0), 0);

  if (isAudio && totalMs > RED_BOOK_MAX_MS) {
    alert("Die ausgewählten Tracks überschreiten das 80-Minuten-Limit für Audio CDs.");
    return;
  }

  if (isData && totalMs * MP3_256K_BYTES_PER_MS > DATA_CD_MAX_BYTES) {
    alert("Das Datenvolumen überschreitet 700 MB.");
    return;
  }

  const driveId = elements.driveSelect.value || state.selectedDriveId;
  const speed = parseInt(elements.burnSpeedSelect.value, 10) || 0;
  const ejectAfter = elements.ejectAfterCheckbox.checked;
  const simulate = elements.simulateCheckbox.checked || driveId.startsWith("sim-");

  state.isBurning = true;
  elements.startBurnBtn.disabled = true;
  elements.burnProgressFill.style.width = "0%";
  elements.burnPctText.textContent = "0%";
  elements.burnStageText.textContent = "Wird gestartet...";
  elements.burnDetailText.textContent = `Initialisiere für ${selectedTracks.length} Tracks...`;

  // Automatically open the Logs modal so the user sees live progress
  openModal("logs");
  elements.logStatusBadge?.classList.add("active");

  logMessage(
    "info",
    `Starte Vorgang: ${selectedTracks.length} Tracks | Modus: ${state.burnMode} | Drive: ${driveId}`
  );

  try {
    await callIpc("start_burn_job", {
      tracks: selectedTracks,
      driveId: state.burnMode === "ExportOnly" ? null : driveId,
      burnMode: state.burnMode,
      speed,
      ejectAfter,
      simulate,
    });
    // Start active polling as fallback to ensure logs and progress always update in real time
    startBurnStatusPolling();
  } catch (err) {
    state.isBurning = false;
    elements.logStatusBadge?.classList.remove("active");
    elements.logStatusBadge?.classList.add("error");
    updateCapacityMeter();
    elements.burnStageText.textContent = "Fehler";
    elements.burnDetailText.textContent = String(err);
    logMessage("error", `Fehler beim Starten: ${err}`);
  }
}

// ==========================================================================
// Modals Handling (Settings & Logs)
// ==========================================================================
function openModal(name) {
  if (name === "settings") {
    elements.settingsModal.style.display = "flex";
    loadConfig();
  } else if (name === "logs") {
    elements.logsModal.style.display = "flex";
    elements.logStatusBadge?.classList.remove("error");
  }
}

function closeModal(name) {
  if (name === "settings") {
    elements.settingsModal.style.display = "none";
  } else if (name === "logs") {
    elements.logsModal.style.display = "none";
  }
}

function setupModals() {
  // Settings Modal openers and closers
  elements.openSettingsBtn?.addEventListener("click", () => openModal("settings"));
  elements.closeSettingsBtn?.addEventListener("click", () => closeModal("settings"));
  elements.cancelSettingsBtn?.addEventListener("click", () => closeModal("settings"));
  elements.settingsModal?.addEventListener("click", (e) => {
    if (e.target === elements.settingsModal) closeModal("settings");
  });

  // Logs Modal openers and closers
  elements.openLogsBtn?.addEventListener("click", () => openModal("logs"));
  elements.closeLogsBtn?.addEventListener("click", () => closeModal("logs"));
  elements.closeLogsFooterBtn?.addEventListener("click", () => closeModal("logs"));
  elements.logsModal?.addEventListener("click", (e) => {
    if (e.target === elements.logsModal) closeModal("logs");
  });

  // Global Escape key to close modals
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      closeModal("settings");
      closeModal("logs");
    }
  });

  // Open cache folder buttons
  elements.modalOpenCacheBtn?.addEventListener("click", async () => {
    try {
      const opened = await callIpc("open_cache_folder");
      logMessage("info", `Cache-Ordner geöffnet: ${opened}`);
    } catch (err) {
      logMessage("error", `Ordner konnte nicht geöffnet werden: ${err}`);
    }
  });

  elements.openDestFolderBtn?.addEventListener("click", async () => {
    try {
      const opened = await callIpc("open_cache_folder");
      logMessage("info", `Zielordner geöffnet: ${opened}`);
    } catch (err) {
      logMessage("error", `Ordner konnte nicht geöffnet werden: ${err}`);
    }
  });

  elements.openExportDirBtn?.addEventListener("click", async () => {
    try {
      const opened = await callIpc("open_cache_folder");
      logMessage("info", `Export-Ordner geöffnet: ${opened}`);
    } catch (err) {
      logMessage("error", `Ordner konnte nicht geöffnet werden: ${err}`);
    }
  });

  // Choose destination folder buttons
  elements.chooseDestFolderBtn?.addEventListener("click", selectDestinationFolder);
  elements.modalChooseCacheBtn?.addEventListener("click", selectDestinationFolder);

  // Toggle client secret password visibility
  elements.toggleSecretBtn?.addEventListener("click", () => {
    const input = elements.clientSecretInput;
    if (input.type === "password") {
      input.type = "text";
      elements.toggleSecretBtn.textContent = "🔒";
    } else {
      input.type = "password";
      elements.toggleSecretBtn.textContent = "👁️";
    }
  });

  // Save config
  elements.saveConfigBtn?.addEventListener("click", saveConfig);
}

// Select Destination Folder via Native OS Dialog
async function selectDestinationFolder() {
  try {
    const selected = await callIpc("select_destination_folder");
    if (selected && selected.trim().length > 0) {
      const cleanPath = selected.trim();
      state.config.cache_dir = cleanPath;
      if (elements.cacheDirInput) elements.cacheDirInput.value = cleanPath;
      if (elements.currentDestFolder) elements.currentDestFolder.textContent = cleanPath;

      await callIpc("save_config", { config: state.config });
      logMessage("success", `✓ Download- & Zielordner festgelegt auf: ${cleanPath}`);
    }
  } catch (err) {
    logMessage("error", `Ordnerauswahl fehlgeschlagen: ${err}`);
  }
}

// Config Load / Save
async function loadConfig() {
  try {
    const config = await callIpc("get_config");
    state.config = config;
    if (elements.clientIdInput) elements.clientIdInput.value = config.client_id || "";
    if (elements.clientSecretInput) elements.clientSecretInput.value = config.client_secret || "";
    if (elements.cacheDirInput) elements.cacheDirInput.value = config.cache_dir || "";
    if (elements.currentDestFolder) {
      elements.currentDestFolder.textContent = config.cache_dir || "~/.spotyburn/cache";
    }
    if (config.default_burn_mode && !state.burnMode) {
      setBurnMode(config.default_burn_mode);
    }
  } catch (err) {
    logMessage("error", `Fehler beim Laden der Konfiguration: ${err}`);
  }
}

async function saveConfig() {
  const updated = {
    client_id: elements.clientIdInput.value.trim(),
    client_secret: elements.clientSecretInput.value.trim(),
    cache_dir: elements.cacheDirInput.value.trim(),
    default_burn_mode: state.burnMode,
  };

  try {
    elements.saveConfigBtn.disabled = true;
    elements.authStatusMsg.textContent = "Speichern...";
    elements.authStatusMsg.className = "status-msg info";

    await callIpc("save_config", { config: updated });
    state.config = updated;
    if (elements.currentDestFolder) {
      elements.currentDestFolder.textContent = updated.cache_dir || "~/.spotyburn/cache";
    }

    elements.authStatusMsg.textContent = "✓ Konfiguration erfolgreich gespeichert!";
    elements.authStatusMsg.className = "status-msg success";
    logMessage("success", "Konfiguration gespeichert.");
    setTimeout(() => {
      elements.authStatusMsg.textContent = "";
      closeModal("settings");
    }, 1200);
  } catch (err) {
    elements.authStatusMsg.textContent = `Fehler: ${err}`;
    elements.authStatusMsg.className = "status-msg error";
    logMessage("error", `Speichern fehlgeschlagen: ${err}`);
  } finally {
    elements.saveConfigBtn.disabled = false;
  }
}

// ==========================================================================
// Setup Native Tauri Event Listeners
// ==========================================================================
function setupTauriEventListeners() {
  if (typeof tauriListen !== "function") {
    console.warn("Tauri event listener not available. Skipping native listeners.");
    return;
  }

  tauriListen("burn-progress", (event) => {
    const payload = event.payload;
    if (!payload) return;

    elements.burnStageText.textContent = payload.stage || "In Bearbeitung";
    const pct = Math.min(Math.max(payload.percent || 0, 0), 100);
    elements.burnPctText.textContent = `${pct.toFixed(0)}%`;
    elements.burnProgressFill.style.width = `${pct}%`;

    if (payload.message) {
      elements.burnDetailText.textContent = payload.message;
    }
  });

  tauriListen("burn-log", (event) => {
    const payload = event.payload;
    if (!payload) return;
    logMessage(payload.level || "info", payload.message || "");
  });

  tauriListen("burn-finished", (event) => {
    stopBurnStatusPolling();
    const payload = event.payload;
    state.isBurning = false;
    elements.logStatusBadge?.classList.remove("active");
    updateCapacityMeter();

    elements.burnStageText.textContent = "Erfolgreich abgeschlossen";
    elements.burnPctText.textContent = "100%";
    elements.burnProgressFill.style.width = "100%";
    elements.burnDetailText.textContent = payload?.message || "Vorgang beendet.";

    logMessage("success", `✓ Vorgang abgeschlossen: ${payload?.message || "Erfolg"}`);
  });

  tauriListen("burn-error", (event) => {
    stopBurnStatusPolling();
    const payload = event.payload;
    state.isBurning = false;
    elements.logStatusBadge?.classList.remove("active");
    elements.logStatusBadge?.classList.add("error");
    updateCapacityMeter();

    elements.burnStageText.textContent = "Fehlgeschlagen";
    elements.burnDetailText.textContent = payload?.error || "Ein Fehler ist aufgetreten.";

    logMessage("error", `❌ Fehler in Phase '${payload?.stage}': ${payload?.error}`);
  });
}

function startBurnStatusPolling() {
  stopBurnStatusPolling();
  state.lastLogIndex = 0;
  state.burnPollingInterval = setInterval(async () => {
    try {
      const status = await callIpc("get_burn_status");
      if (!status) return;

      // Update progress UI
      if (status.stage) {
        elements.burnStageText.textContent = status.stage;
      }
      const pct = Math.min(Math.max(status.percent || 0, 0), 100);
      elements.burnPctText.textContent = `${pct.toFixed(0)}%`;
      elements.burnProgressFill.style.width = `${pct}%`;

      if (status.message) {
        elements.burnDetailText.textContent = status.message;
      }

      // Stream any new logs
      if (Array.isArray(status.logs) && status.logs.length > state.lastLogIndex) {
        for (let i = state.lastLogIndex; i < status.logs.length; i++) {
          const logEntry = status.logs[i];
          logMessage(logEntry.level || "info", logEntry.message || "");
        }
        state.lastLogIndex = status.logs.length;
      }

      // Check if finished or errored
      if (status.finished) {
        stopBurnStatusPolling();
        state.isBurning = false;
        elements.logStatusBadge?.classList.remove("active");
        updateCapacityMeter();
        elements.burnStageText.textContent = "Erfolgreich abgeschlossen";
        elements.burnPctText.textContent = "100%";
        elements.burnProgressFill.style.width = "100%";
        elements.burnDetailText.textContent = status.finished.message || "Vorgang beendet.";
        logMessage("success", `✓ Vorgang abgeschlossen: ${status.finished.message || "Erfolg"}`);
      } else if (status.error) {
        stopBurnStatusPolling();
        state.isBurning = false;
        elements.logStatusBadge?.classList.remove("active");
        elements.logStatusBadge?.classList.add("error");
        updateCapacityMeter();
        elements.burnStageText.textContent = "Fehlgeschlagen";
        elements.burnDetailText.textContent = status.error.error || "Ein Fehler ist aufgetreten.";
        logMessage("error", `❌ Fehler in Phase '${status.error.stage}': ${status.error.error}`);
      } else if (!status.is_active && pct >= 100) {
        stopBurnStatusPolling();
      }
    } catch (e) {
      console.warn("Burn status polling error:", e);
    }
  }, 400);
}

function stopBurnStatusPolling() {
  if (state.burnPollingInterval) {
    clearInterval(state.burnPollingInterval);
    state.burnPollingInterval = null;
  }
}

// ==========================================================================
// Main Initialization
// ==========================================================================
window.addEventListener("DOMContentLoaded", async () => {
  initElements();
  setupModals();
  setupSearchHandlers();

  // Spotify Auth Events
  elements.spotifyLoginBtn?.addEventListener("click", loginSpotify);
  elements.sidebarLoginBtn?.addEventListener("click", loginSpotify);
  elements.spotifyLogoutBtn?.addEventListener("click", logoutSpotify);
  elements.refreshPlaylistsBtn?.addEventListener("click", loadUserPlaylists);

  // URL toggle
  elements.toggleUrlBtn?.addEventListener("click", () => {
    const isOpen = elements.urlInputCollapse.style.display !== "none";
    elements.urlInputCollapse.style.display = isOpen ? "none" : "flex";
    elements.urlToggleChevron.classList.toggle("open", !isOpen);
  });
  elements.fetchTracksBtn?.addEventListener("click", handleManualUrlFetch);
  elements.spotifyUrlInput?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") handleManualUrlFetch();
  });

  // Track Table Selection Events
  elements.selectAllBtn?.addEventListener("click", () => selectAllTracks(true));
  elements.selectNoneBtn?.addEventListener("click", () => selectAllTracks(false));
  elements.clearTracksBtn?.addEventListener("click", clearAllTracks);
  elements.masterTrackCheckbox?.addEventListener("change", (e) => {
    selectAllTracks(e.target.checked);
  });

  // Mode Tabs Switching
  elements.modeTabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      setBurnMode(tab.dataset.mode);
    });
  });

  // Drive Controls Events
  elements.refreshDrivesBtn?.addEventListener("click", loadOpticalDrives);
  elements.driveSelect?.addEventListener("change", checkMediaStatus);
  elements.checkMediaBtn?.addEventListener("click", checkMediaStatus);
  elements.ejectDriveBtn?.addEventListener("click", ejectDrive);

  // Burn Execution Event
  elements.startBurnBtn?.addEventListener("click", startBurnJob);

  // Console Clear
  elements.clearLogsBtn?.addEventListener("click", () => {
    elements.consoleWindow.innerHTML = "";
    logMessage("info", "Konsole zurückgesetzt.");
  });

  // Setup streaming event bridge
  setupTauriEventListeners();

  // Load initial configuration, check profile & drives
  await loadConfig();
  await checkUserProfile();
  await loadOpticalDrives();
  updateCapacityMeter();

  logMessage("info", "SpotyBurn v2.0 Dashboard bereit.");
});
