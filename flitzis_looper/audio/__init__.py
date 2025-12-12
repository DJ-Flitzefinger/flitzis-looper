"""Audio package for flitzis_looper.

Contains pyo server initialization, loop player, BPM detection, and stem separation.
"""

from flitzis_looper.audio.bpm import _detect_bpm_worker
from flitzis_looper.audio.loop import PyoLoop
from flitzis_looper.audio.pitch import (
    _create_pitched_stem_cache,
    cleanup_stem_caches,
    invalidate_stem_caches,
    precache_pitched_stems_if_needed,
)
from flitzis_looper.audio.server import (
    get_master_amp,
    get_server,
    init_master_amp,
    init_server,
    set_master_amp,
    shutdown_server,
)
from flitzis_looper.audio.stems_engine import (
    _activate_main_loop,
    _activate_stem_players,
    _build_stem_mix_and_eq,
    _cleanup_stem_players,
    _create_main_table_from_audio,
    _create_stem_table_from_audio,
    _initialize_stems_while_running,
    _restart_stem_phasor,
    _select_main_audio_for_stems,
    _select_stem_audio,
    apply_stem_mix,
    initialize_stem_players,
    stop_stem_players,
    update_stem_eq,
    update_stem_gains,
)
from flitzis_looper.audio.stems_separation import (
    delete_stems,
    generate_stems,
)

__all__ = [
    # Loop
    "PyoLoop",
    "_activate_main_loop",
    "_activate_stem_players",
    "_build_stem_mix_and_eq",
    "_cleanup_stem_players",
    "_create_main_table_from_audio",
    # Pitch
    "_create_pitched_stem_cache",
    "_create_stem_table_from_audio",
    # BPM
    "_detect_bpm_worker",
    "_initialize_stems_while_running",
    "_restart_stem_phasor",
    "_select_main_audio_for_stems",
    "_select_stem_audio",
    "apply_stem_mix",
    "cleanup_stem_caches",
    "delete_stems",
    # Stems Separation
    "generate_stems",
    "get_master_amp",
    "get_server",
    "init_master_amp",
    # Server
    "init_server",
    # Stems Engine
    "initialize_stem_players",
    "invalidate_stem_caches",
    "precache_pitched_stems_if_needed",
    "set_master_amp",
    "shutdown_server",
    "stop_stem_players",
    "update_stem_eq",
    "update_stem_gains",
]
