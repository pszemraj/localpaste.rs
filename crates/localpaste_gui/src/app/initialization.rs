//! App startup and initial session state.

use super::*;

impl LocalPasteApp {
    /// Construct a new app instance from the current environment config.
    ///
    /// Opens the embedded database, spawns the backend worker thread, and kicks
    /// off the initial list request so the UI has data to render on first paint.
    ///
    /// # Returns
    /// The initialized [`LocalPasteApp`] ready to be handed to `eframe`.
    ///
    /// # Errors
    /// Returns an error if the database path is invalid or the underlying store
    /// cannot be opened.
    pub(crate) fn new() -> Result<Self, localpaste_core::AppError> {
        let config = Config::from_env();
        let db_path = config.db_path.clone();
        let autosave_delay = Duration::from_millis(config.auto_save_interval);
        let db = Database::new(&config.db_path)?;
        let version_history_limit = db.paste_version_retention_limit();
        info!("native GUI opened database at {}", config.db_path);

        let locks = Arc::new(PasteLockManager::default());
        let server_db = db.share()?;
        let state = AppState::with_locks(config.clone(), server_db, locks.clone());
        let allow_public = localpaste_core::config::env_flag_enabled("ALLOW_PUBLIC_ACCESS");
        if allow_public {
            warn!("Public access enabled - server will accept requests from any origin");
        }
        let server = EmbeddedServer::start(state, allow_public)?;
        let server_addr = server.addr();
        let server_used_fallback = server.used_fallback();

        let lock_owner_id = crate::lock_owner::next_lock_owner_id("gui");
        let backend = spawn_backend_with_locks_and_owner(
            db,
            config.max_paste_size,
            locks.clone(),
            lock_owner_id.clone(),
        );
        let highlight_worker = spawn_highlight_worker();

        let mut app = Self {
            backend,
            all_pastes: Vec::new(),
            pastes: Vec::new(),
            selected_id: None,
            selected_paste: None,
            edit_name: String::new(),
            edit_language: None,
            edit_language_is_manual: false,
            edit_tags: String::new(),
            metadata_dirty: false,
            metadata_save_in_flight: false,
            metadata_save_request: None,
            editor_find: EditorFindState::default(),
            search_query: String::new(),
            search_scope: SearchScope::All,
            search_sent_scope: SearchScope::All,
            search_last_input_at: None,
            search_last_sent: String::new(),
            search_focus_requested: false,
            active_collection: SidebarCollection::All,
            active_language_filter: None,
            properties_drawer_open: false,
            command_palette_open: false,
            command_palette_query: String::new(),
            command_palette_selected: 0,
            paste_picker_open: false,
            paste_picker_query: String::new(),
            paste_picker_selected: 0,
            paste_picker_scroll_reset_pending: false,
            paste_picker_scope: SearchScope::All,
            paste_picker_sent_scope: SearchScope::All,
            palette_search_results: Vec::new(),
            palette_search_last_sent: String::new(),
            palette_search_last_input_at: None,
            palette_search_pending: false,
            pending_copy_action: None,
            pending_selection_id: None,
            pending_picker_open: None,
            picker_selection_pin: None,
            pending_delete_id: None,
            clipboard_outgoing: None,
            active_buffer_epoch: 0,
            virtual_editor_buffer: RopeBuffer::new(""),
            virtual_editor_state: VirtualEditorState::default(),
            virtual_editor_history: VirtualEditorHistory::default(),
            virtual_layout: WrapLayoutCache::default(),
            virtual_galley_cache: VirtualGalleyCache::default(),
            virtual_line_scratch: String::new(),
            virtual_caret_phase_start: Instant::now(),
            virtual_drag_active: false,
            virtual_viewport_height: 0.0,
            virtual_line_height: 1.0,
            virtual_wrap_width: 0.0,
            virtual_pending_scroll_offset_y: None,
            virtual_cursor_reveal: None,
            virtual_viewport: EditorViewport::default(),
            virtual_paste_applied_this_frame: false,
            version_history_limit,
            version_ui: VersionUiState::default(),
            highlight_worker,
            highlight_pending: None,
            highlight_render: None,
            highlight_staged: None,
            highlight_staged_invalidation: None,
            highlight_version: 0,
            highlight_edit_hint: None,
            db_path,
            locks,
            lock_owner_id,
            _server: server,
            server_addr,
            server_used_fallback,
            status: None,
            toasts: VecDeque::with_capacity(TOAST_LIMIT),
            pending_undo_restore_tokens: HashSet::new(),
            export_result_rx: None,
            save_status: SaveStatus::Saved,
            last_edit_at: None,
            save_in_flight: false,
            save_request_revision: None,
            autosave_delay,
            shortcut_help_open: false,
            shortcut_help_query: String::new(),
            shortcut_help_focus_requested: false,
            shortcut_help_return_focus: None,
            focus_editor_next: false,
            style_applied: false,
            window_shown_once: false,
            window_checked: false,
            last_refresh_at: Instant::now(),
            backend_event_poll_until: None,
            query_perf: QueryPerfCounters::default(),
            perf_log_enabled: env_flag_enabled("LOCALPASTE_EDITOR_PERF_LOG"),
            frame_samples: VecDeque::with_capacity(PERF_SAMPLE_CAP),
            last_frame_at: None,
            last_perf_log_at: Instant::now(),
            last_interaction_at: None,
            last_virtual_click_at: None,
            last_virtual_click_pos: None,
            last_virtual_click_count: 0,
            paste_as_new_pending_frames: 0,
            paste_as_new_clipboard_requested_at: None,
            editor_input_trace_enabled: env_flag_enabled("LOCALPASTE_EDITOR_INPUT_TRACE"),
            highlight_trace_enabled: env_flag_enabled("LOCALPASTE_HIGHLIGHT_TRACE"),
            nav_probe: nav_probe::NavProbe::from_env(),
            nav_probe_applied_commands: Vec::new(),
        };
        if !app.apply_nav_probe_seed_from_env() {
            app.request_refresh();
        }
        Ok(app)
    }
}
