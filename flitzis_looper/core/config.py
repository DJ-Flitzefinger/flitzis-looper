"""ConfigManager - Konfigurationsverwaltung für flitzis_looper.

Kapselt das Laden und Speichern der Konfiguration in einer Klasse.
"""

from __future__ import annotations

import contextlib
import json
import logging
import os
import tkinter as tk
from typing import TYPE_CHECKING, Any

from flitzis_looper.core.state import (
    NUM_BANKS,
    ensure_stems_structure,
    get_all_banks_data,
    get_current_bank,
    get_master_amp,
    get_master_volume,
    get_root,
    set_button_data_ref,
)
from flitzis_looper.utils.paths import CONFIG_FILE
from flitzis_looper.utils.threading import io_executor

if TYPE_CHECKING:
    from collections.abc import Callable

logger = logging.getLogger(__name__)


class ConfigManager:
    """Verwaltet die Konfiguration der Anwendung.

    Singleton-Pattern für globalen Zugriff.
    Unterstützt Debounced Saving für Performance.

    Example:
        >>> config = ConfigManager.instance()
        >>> config.load(register_loop, PyoLoop)
        >>> config.save_async()  # Debounced
        >>> config.save_immediate()  # Sofort
    """

    _instance: ConfigManager | None = None

    # Konfigurierbare Felder die gespeichert werden
    SAVED_FIELDS = [
        "file",
        "bpm",
        "gain_db",
        "loop_start",
        "loop_end",
        "auto_loop_active",
        "auto_loop_bars",
        "auto_loop_custom_mode",
        "intro_active",
        "intro_bars",
        "intro_custom_mode",
        "eq_low",
        "eq_mid",
        "eq_high",
    ]

    # Felder die NICHT gespeichert werden (nur RAM)
    IGNORED_FIELDS = ["stems_available", "stems_states", "pyo", "stems", "waveform_cache"]

    def __init__(self, config_path: str = CONFIG_FILE, debounce_ms: int = 2000):
        """Initialisiert den ConfigManager.

        Args:
            config_path: Pfad zur Config-Datei
            debounce_ms: Debounce-Zeit in Millisekunden
        """
        self._config_path = config_path
        self._debounce_ms = debounce_ms
        self._save_timer: str | None = None

    @classmethod
    def instance(cls) -> ConfigManager:
        """Gibt die Singleton-Instanz zurück."""
        if cls._instance is None:
            cls._instance = ConfigManager()
        return cls._instance

    @classmethod
    def reset(cls) -> None:
        """Setzt die Instanz zurück (für Tests)."""
        cls._instance = None

    @property
    def config_path(self) -> str:
        """Gibt den Pfad zur Config-Datei zurück."""
        return self._config_path

    def save(self) -> bool:
        """Speichert die Konfiguration synchron.

        Returns:
            True bei Erfolg, False bei Fehler
        """
        try:
            all_banks_data = get_all_banks_data()
            master_volume = get_master_volume()

            config = {
                "global": {"master_volume": master_volume.get()},
                "banks": {},
            }

            for bank_id in range(1, NUM_BANKS + 1):
                config["banks"][str(bank_id)] = {}

                for btn_id, data in all_banks_data[bank_id].items():
                    if data.get("file"):
                        btn_config = {}

                        # Nur definierte Felder speichern
                        for field in self.SAVED_FIELDS:
                            if field in data:
                                btn_config[field] = data[field]

                        # Defaults für optionale Felder
                        btn_config.setdefault("loop_start", 0.0)
                        btn_config.setdefault("auto_loop_active", True)
                        btn_config.setdefault("auto_loop_bars", 8)
                        btn_config.setdefault("auto_loop_custom_mode", False)
                        btn_config.setdefault("intro_active", False)
                        btn_config.setdefault("intro_bars", 4)
                        btn_config.setdefault("intro_custom_mode", False)
                        btn_config.setdefault("eq_low", 0.0)
                        btn_config.setdefault("eq_mid", 0.0)
                        btn_config.setdefault("eq_high", 0.0)

                        config["banks"][str(bank_id)][str(btn_id)] = btn_config

            with open(self._config_path, "w", encoding="utf-8") as f:
                json.dump(config, f, indent=2)

            logger.debug("Config saved to %s", self._config_path)

        except Exception:
            logger.exception("Error saving config")
            return False

        return True

    def save_async(self) -> None:
        """Speichert die Konfiguration asynchron mit Debouncing.

        Wartet `debounce_ms` Millisekunden vor dem Speichern.
        Mehrere Aufrufe in kurzer Zeit werden zusammengefasst.
        """
        root = get_root()
        if root is None:
            # Fallback: Synchron speichern
            io_executor.submit(self.save)
            return

        # Alten Timer abbrechen
        if self._save_timer is not None:
            with contextlib.suppress(ValueError, tk.TclError):
                root.after_cancel(self._save_timer)

        # Neuen Timer setzen
        self._save_timer = root.after(
            self._debounce_ms,
            lambda: io_executor.submit(self.save),
        )

    def save_immediate(self) -> None:
        """Speichert sofort ohne Debouncing.

        Für wichtige Änderungen wie beim Schließen der App.
        """
        root = get_root()

        # Pending Timer abbrechen
        if self._save_timer is not None:
            try:
                if root:
                    root.after_cancel(self._save_timer)
            except (ValueError, tk.TclError):
                pass
            self._save_timer = None

        # Sofort speichern (im Thread)
        io_executor.submit(self.save)

    def load(
        self,
        register_loaded_loop: Callable[[int, int, Any], None],
        pyoloop_class: type,
    ) -> bool:
        """Lädt die Konfiguration.

        Args:
            register_loaded_loop: Callback zum Registrieren geladener Loops
            pyoloop_class: Die PyoLoop-Klasse zum Erstellen neuer Loops

        Returns:
            True bei Erfolg, False bei Fehler
        """
        if not os.path.exists(self._config_path):
            logger.debug("Config file not found: %s", self._config_path)
            return False

        try:
            with open(self._config_path, encoding="utf-8") as f:
                config = json.load(f)

            self._apply_global_config(config)
            self._apply_banks_config(config, register_loaded_loop, pyoloop_class)

            logger.debug("Config loaded from %s", self._config_path)

        except Exception:
            logger.exception("Error loading config")
            return False

        return True

    def _apply_global_config(self, config: dict) -> None:
        """Wendet globale Konfiguration an."""
        if "global" not in config:
            return

        master_volume = get_master_volume()
        master_amp = get_master_amp()

        volume = config["global"].get("master_volume", 1.0)
        master_volume.set(volume)

        if master_amp:
            master_amp.value = volume

    def _apply_banks_config(
        self,
        config: dict,
        register_loaded_loop: Callable[[int, int, Any], None],
        pyoloop_class: type,
    ) -> None:
        """Wendet Bank-Konfiguration an."""
        if "banks" not in config:
            return

        all_banks_data = get_all_banks_data()
        current_bank = get_current_bank()

        for bank_id_str, bank_config in config["banks"].items():
            bank_id = int(bank_id_str)

            for btn_id_str, btn_config in bank_config.items():
                btn_id = int(btn_id_str)

                # Prüfe ob Datei existiert
                file_path = btn_config.get("file")
                if not file_path or not os.path.exists(file_path):
                    continue

                data = all_banks_data[bank_id][btn_id]

                # Config-Werte übernehmen (außer ignorierte)
                for key, value in btn_config.items():
                    if key not in self.IGNORED_FIELDS:
                        data[key] = value

                # Stems-Struktur sicherstellen
                ensure_stems_structure(data)

                # Loop laden
                loop = pyoloop_class()
                if loop.load(
                    data["file"],
                    data.get("loop_start", 0.0),
                    data.get("loop_end"),
                ):
                    loop.set_gain(data.get("gain_db", 0.0))
                    loop.set_eq(
                        data.get("eq_low", 0.0),
                        data.get("eq_mid", 0.0),
                        data.get("eq_high", 0.0),
                    )
                    data["pyo"] = loop
                    register_loaded_loop(bank_id, btn_id, loop)

        # button_data auf aktuelle Bank setzen
        set_button_data_ref(current_bank.get())


# ============== SINGLETON INSTANCE ==============
_config_manager: ConfigManager | None = None


def get_config_manager() -> ConfigManager:
    """Gibt die globale ConfigManager-Instanz zurück."""
    return ConfigManager.instance()


# ============== BACKWARD-COMPATIBLE FUNCTIONS ==============
# Diese Funktionen existieren für Backward-Kompatibilität.


def save_config() -> None:
    """Speichert die Konfiguration (Backward-kompatibel)."""
    ConfigManager.instance().save()


def save_config_async() -> None:
    """Speichert die Konfiguration asynchron (Backward-kompatibel)."""
    ConfigManager.instance().save_async()


def save_config_immediate() -> None:
    """Speichert die Konfiguration sofort (Backward-kompatibel)."""
    ConfigManager.instance().save_immediate()


def load_config(register_loaded_loop_callback, pyoloop_class) -> None:
    """Lädt die Konfiguration (Backward-kompatibel)."""
    ConfigManager.instance().load(register_loaded_loop_callback, pyoloop_class)
