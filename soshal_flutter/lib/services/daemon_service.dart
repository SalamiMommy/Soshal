import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../ffi/daemon.dart' as ffi_daemon;

/// Manager for bundled networking daemons (I2P, Freenet, Reticulum).
///
/// Thin wrapper over the Rust bridge (ffi/daemon.rs): asset extraction,
/// spawn, stop, and liveness all live in Rust. Only JSON-to-model mapping
/// happens here.
class DaemonService {
  /// Extracts bundled daemons from assets to the app data directory.
  static Future<bool> extractDaemons() async {
    try {
      return ffi_daemon.daemonExtractDaemons();
    } catch (e) {
      debugPrint('Failed to extract daemons: $e');
      return false;
    }
  }

  /// Gets the path to a specific daemon binary (empty if missing).
  static Future<String?> getDaemonPath(String daemonName) async {
    try {
      final path = ffi_daemon.daemonGetDaemonPath(daemonName: daemonName);
      return path.isEmpty ? null : path;
    } catch (e) {
      debugPrint('Failed to get daemon path: $e');
      return null;
    }
  }

  /// Checks if all daemons are available.
  static Future<bool> areDaemonsAvailable() async {
    try {
      return ffi_daemon.daemonAreDaemonsAvailable();
    } catch (e) {
      debugPrint('Failed to check daemon availability: $e');
      return false;
    }
  }

  /// Gets daemon status information.
  static Future<Map<String, bool>> getDaemonStatus() async {
    try {
      final json = ffi_daemon.daemonGetDaemonStatus();
      final map = jsonDecode(json) as Map<String, dynamic>;
      return map.map((k, v) => MapEntry(k, v as bool));
    } catch (e) {
      debugPrint('Failed to get daemon status: $e');
      return {};
    }
  }

  /// Extracts bundled daemons and spawns i2pd (SAM 7656 + SOCKS 4447).
  static Future<bool> startDaemons() async {
    try {
      return ffi_daemon.daemonStartDaemons();
    } catch (e) {
      debugPrint('Failed to start daemons: $e');
      return false;
    }
  }

  /// Stops the spawned daemon processes.
  static Future<bool> stopDaemons() async {
    try {
      return ffi_daemon.daemonStopDaemons();
    } catch (e) {
      debugPrint('Failed to stop daemons: $e');
      return false;
    }
  }

  /// Process liveness of the spawned i2pd (not just binary existence).
  static Future<bool> isI2pdRunning() async {
    try {
      return ffi_daemon.daemonIsI2PdRunning();
    } catch (e) {
      debugPrint('Failed to check i2pd liveness: $e');
      return false;
    }
  }

  /// Process liveness of the spawned Reticulum daemon.
  static Future<bool> isRnsdRunning() async {
    try {
      return ffi_daemon.daemonIsRnsdRunning();
    } catch (e) {
      debugPrint('Failed to check rnsd liveness: $e');
      return false;
    }
  }

  /// Daemon names
  static const String i2pd = 'i2pd';
  static const String freenet = 'freenet';
  static const String reticulum = 'rnsd';

  /// Gets I2P daemon path
  static Future<String?> getI2PPath() => getDaemonPath(i2pd);

  /// Gets Freenet daemon path
  static Future<String?> getFreenetPath() => getDaemonPath(freenet);

  /// Gets Reticulum daemon path
  static Future<String?> getReticulumPath() => getDaemonPath(reticulum);
}

/// Daemon status information
class DaemonStatus {
  final bool i2pdAvailable;
  final bool freenetAvailable;
  final bool reticulumAvailable;

  DaemonStatus({
    required this.i2pdAvailable,
    required this.freenetAvailable,
    required this.reticulumAvailable,
  });

  factory DaemonStatus.fromMap(Map<String, bool> map) {
    return DaemonStatus(
      i2pdAvailable: map['i2pd'] ?? false,
      freenetAvailable: map['freenet'] ?? false,
      reticulumAvailable: map['reticulum'] ?? false,
    );
  }

  bool get allAvailable =>
      i2pdAvailable && freenetAvailable && reticulumAvailable;

  int get availableCount => [
        i2pdAvailable,
        freenetAvailable,
        reticulumAvailable
      ].where((e) => e).length;
}
