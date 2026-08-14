// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// eBPF Traffic Shaper Service
/// Controls kernel socket packet filtering, rate limiting, and nanosecond
/// spam drop statistics.
class EbpfService extends ChangeNotifier {
  final bool _enabled = true;
  String _mode = 'SocketFilterBpf';
  int _droppedPackets = 0;
  int _passedPackets = 0;
  int _nanosSaved = 0;
  int _blockedPeersCount = 0;
  String? _lastError;

  bool get enabled => _enabled;
  String get mode => _mode;
  int get droppedPackets => _droppedPackets;
  int get passedPackets => _passedPackets;
  int get nanosSaved => _nanosSaved;
  int get blockedPeersCount => _blockedPeersCount;
  String? get lastError => _lastError;

  /// Block an IP address in the kernel / socket filter
  Future<bool> blockIp(String ip) async {
    try {
      final res = RustLib.instance.api.crateFfiEbpfEbpfBlockIp(ip: ip);
      await refreshStats();
      return res;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      return false;
    }
  }

  /// Unblock an IP address in the kernel / socket filter
  Future<bool> unblockIp(String ip) async {
    try {
      final res = RustLib.instance.api.crateFfiEbpfEbpfUnblockIp(ip: ip);
      await refreshStats();
      return res;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      return false;
    }
  }

  /// Refresh real-time eBPF shaper diagnostics from Rust core
  Future<void> refreshStats() async {
    try {
      final jsonStr = RustLib.instance.api.crateFfiEbpfEbpfGetStats();
      final Map<String, dynamic> stats =
          Map<String, dynamic>.from(jsonDecode(jsonStr) as Map);

      _mode = stats['mode']?.toString() ?? 'SocketFilterBpf';
      _droppedPackets = (stats['dropped_packets'] as num?)?.toInt() ?? 0;
      _passedPackets = (stats['passed_packets'] as num?)?.toInt() ?? 0;
      _nanosSaved = (stats['nanos_saved'] as num?)?.toInt() ?? 0;
      _blockedPeersCount = (stats['blocked_peers_count'] as num?)?.toInt() ?? 0;
      _lastError = null;
      notifyListeners();
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
    }
  }
}
