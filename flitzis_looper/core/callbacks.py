"""CallbackRegistry - Zentrale Callback-Verwaltung für flitzis_looper.

Ersetzt die manuellen Callback-Dicts durch eine zentrale Registry.
Ermöglicht lose Kopplung zwischen Modulen.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from collections.abc import Callable

logger = logging.getLogger(__name__)


class CallbackRegistry:
    """Zentrale Registry für Callbacks.

    Ermöglicht das Registrieren und Aufrufen von Callbacks ohne direkte Imports.
    Singleton-Pattern für globalen Zugriff.

    Example:
        >>> registry = CallbackRegistry.instance()
        >>> registry.register("update_button_label", my_update_func)
        >>> registry.call("update_button_label", button_id=5)
    """

    _instance: CallbackRegistry | None = None

    def __init__(self):
        """Initialisiert die Registry."""
        self._callbacks: dict[str, Callable] = {}
        self._groups: dict[str, list[str]] = {}

    @classmethod
    def instance(cls) -> CallbackRegistry:
        """Gibt die Singleton-Instanz zurück."""
        if cls._instance is None:
            cls._instance = CallbackRegistry()
        return cls._instance

    @classmethod
    def reset(cls) -> None:
        """Setzt die Registry zurück (für Tests)."""
        cls._instance = None

    def register(self, name: str, callback: Callable, group: str | None = None) -> None:
        """Registriert einen Callback.

        Args:
            name: Eindeutiger Name für den Callback
            callback: Die Callback-Funktion
            group: Optionale Gruppe für gebündelte Callbacks
        """
        self._callbacks[name] = callback
        if group:
            if group not in self._groups:
                self._groups[group] = []
            if name not in self._groups[group]:
                self._groups[group].append(name)

    def unregister(self, name: str) -> None:
        """Entfernt einen Callback.

        Args:
            name: Name des zu entfernenden Callbacks
        """
        self._callbacks.pop(name, None)
        # Aus allen Gruppen entfernen
        for group_names in self._groups.values():
            if name in group_names:
                group_names.remove(name)

    def get(self, name: str) -> Callable | None:
        """Gibt einen Callback zurück.

        Args:
            name: Name des Callbacks

        Returns:
            Die Callback-Funktion oder None
        """
        return self._callbacks.get(name)

    def call(self, name: str, *args, **kwargs) -> Any:
        """Ruft einen Callback auf.

        Args:
            name: Name des Callbacks
            *args: Positionale Argumente
            **kwargs: Keyword-Argumente

        Returns:
            Rückgabewert des Callbacks oder None wenn nicht registriert
        """
        callback = self._callbacks.get(name)
        if callback:
            try:
                return callback(*args, **kwargs)
            except Exception:
                logger.exception("Error calling callback '%s'", name)
                return None
        else:
            logger.debug("Callback '%s' not registered", name)
            return None

    def call_if_exists(self, name: str, *args, **kwargs) -> Any:
        """Ruft einen Callback auf, wenn er existiert (ohne Warnung).

        Args:
            name: Name des Callbacks
            *args: Positionale Argumente
            **kwargs: Keyword-Argumente

        Returns:
            Rückgabewert des Callbacks oder None
        """
        callback = self._callbacks.get(name)
        if callback:
            try:
                return callback(*args, **kwargs)
            except Exception:
                logger.exception("Error calling callback '%s'", name)
        return None

    def has(self, name: str) -> bool:
        """Prüft ob ein Callback registriert ist.

        Args:
            name: Name des Callbacks

        Returns:
            True wenn registriert
        """
        return name in self._callbacks

    def get_group(self, group: str) -> dict[str, Callable]:
        """Gibt alle Callbacks einer Gruppe als Dict zurück.

        Args:
            group: Name der Gruppe

        Returns:
            Dict mit {name: callback} für alle Callbacks der Gruppe
        """
        result = {}
        for name in self._groups.get(group, []):
            if name in self._callbacks:
                result[name] = self._callbacks[name]
        return result

    def get_group_dict(self, group: str) -> dict[str, Callable]:
        """Alias für get_group - für Backward-Kompatibilität mit Callback-Dicts.

        Args:
            group: Name der Gruppe

        Returns:
            Dict mit {name: callback}
        """
        return self.get_group(group)

    def list_callbacks(self) -> list[str]:
        """Gibt alle registrierten Callback-Namen zurück."""
        return list(self._callbacks.keys())

    def list_groups(self) -> list[str]:
        """Gibt alle Gruppen-Namen zurück."""
        return list(self._groups.keys())


# ============== CONVENIENCE FUNCTIONS ==============


def get_registry() -> CallbackRegistry:
    """Gibt die globale CallbackRegistry-Instanz zurück."""
    return CallbackRegistry.instance()


def register_callback(name: str, callback: Callable, group: str | None = None) -> None:
    """Registriert einen Callback in der globalen Registry.

    Args:
        name: Eindeutiger Name
        callback: Die Callback-Funktion
        group: Optionale Gruppe
    """
    CallbackRegistry.instance().register(name, callback, group)


def call_callback(name: str, *args, **kwargs) -> Any:
    """Ruft einen Callback aus der globalen Registry auf.

    Args:
        name: Name des Callbacks
        *args: Positionale Argumente
        **kwargs: Keyword-Argumente

    Returns:
        Rückgabewert des Callbacks
    """
    return CallbackRegistry.instance().call(name, *args, **kwargs)


def get_callback(name: str) -> Callable | None:
    """Gibt einen Callback aus der globalen Registry zurück.

    Args:
        name: Name des Callbacks

    Returns:
        Die Callback-Funktion oder None
    """
    return CallbackRegistry.instance().get(name)


def get_callback_group(group: str) -> dict[str, Callable]:
    """Gibt alle Callbacks einer Gruppe als Dict zurück.

    Args:
        group: Name der Gruppe

    Returns:
        Dict mit {name: callback}
    """
    return CallbackRegistry.instance().get_group(group)
