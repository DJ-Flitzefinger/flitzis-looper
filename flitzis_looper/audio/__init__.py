"""
Audio package for flitzis_looper.
Contains pyo server initialization, loop player, BPM detection, and stem separation.
"""

from flitzis_looper.audio.server import (
    init_server,
    get_server,
    init_master_amp,
    get_master_amp,
    set_master_amp,
    shutdown_server,
)

from flitzis_looper.audio.loop import PyoLoop

from flitzis_looper.audio.bpm import _detect_bpm_worker

from flitzis_looper.audio.stems_engine import (
    initialize_stem_players,
    stop_stem_players,
    update_stem_gains,
    update_stem_eq,
    apply_stem_mix,
    _cleanup_stem_players,
    _build_stem_mix_and_eq,
    _create_main_table_from_audio,
    _create_stem_table_from_audio,
    _select_stem_audio,
    _select_main_audio_for_stems,
    _initialize_stems_while_running,
    _activate_main_loop,
    _activate_stem_players,
    _restart_stem_phasor,
)

from flitzis_looper.audio.stems_separation import (
    generate_stems,
    delete_stems,
)

from flitzis_looper.audio.pitch import (
    _create_pitched_stem_cache,
    precache_pitched_stems_if_needed,
    invalidate_stem_caches,
    cleanup_stem_caches,
)

__all__ = [
    # Server
    'init_server',
    'get_server',
    'init_master_amp',
    'get_master_amp',
    'set_master_amp',
    'shutdown_server',
    # Loop
    'PyoLoop',
    # BPM
    '_detect_bpm_worker',
    # Stems Engine
    'initialize_stem_players',
    'stop_stem_players',
    'update_stem_gains',
    'update_stem_eq',
    'apply_stem_mix',
    '_cleanup_stem_players',
    '_build_stem_mix_and_eq',
    '_create_main_table_from_audio',
    '_create_stem_table_from_audio',
    '_select_stem_audio',
    '_select_main_audio_for_stems',
    '_initialize_stems_while_running',
    '_activate_main_loop',
    '_activate_stem_players',
    '_restart_stem_phasor',
    # Stems Separation
    'generate_stems',
    'delete_stems',
    # Pitch
    '_create_pitched_stem_cache',
    'precache_pitched_stems_if_needed',
    'invalidate_stem_caches',
    'cleanup_stem_caches',
]
