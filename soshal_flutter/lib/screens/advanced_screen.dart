import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/error_log.dart';
import '../services/network_service.dart';

/// Advanced settings — relay flags (read/write toggles), diagnostics, and
/// transport probes. Port of the legacy advanced section; relay flag
/// enforcement is backend-side.
class AdvancedScreen extends StatefulWidget {
  const AdvancedScreen({super.key});

  @override
  State<AdvancedScreen> createState() => _AdvancedScreenState();
}

class _AdvancedScreenState extends State<AdvancedScreen> {
  Map<String, dynamic>? _diagnostics;
  bool _i2p = false;
  bool _freenet = false;
  String? _diagError;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final networkService = context.read<NetworkService>();
    try {
      final diag = networkService.fetchSysDiagnostics();
      _diagnostics = jsonDecode(diag) as Map<String, dynamic>?;
      _diagError = null;
    } catch (e) {
      _diagError = '$e';
    }
    try {
      await networkService.refresh();
      _i2p = networkService.i2p ?? false;
      _freenet = networkService.freenet ?? false;
    } catch (e, st) {
      debugPrint('network refresh failed: $e');
      logRuntimeError('network refresh: $e', st);
    }
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Advanced')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text('Transports', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 4),
          ListTile(
            leading: Icon(
              _i2p ? Icons.check_circle : Icons.circle_outlined,
              color: _i2p ? Colors.green : null,
            ),
            title: const Text('I2P tunnel (local i2pd SOCKS 7656)'),
          ),
          ListTile(
            leading: Icon(
              _freenet ? Icons.check_circle : Icons.circle_outlined,
              color: _freenet ? Colors.green : null,
            ),
            title: const Text('Freenet gateway (local port 8888)'),
          ),
          const Divider(),
          Text('Diagnostics', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 4),
          if (_diagError != null)
            Text('Unavailable: $_diagError')
          else if (_diagnostics != null)
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Theme.of(context).colorScheme.surfaceContainerHighest,
                borderRadius: BorderRadius.circular(8),
              ),
              child: SelectableText(
                const JsonEncoder.withIndent('  ').convert(_diagnostics),
                style: const TextStyle(fontSize: 12),
              ),
            ),
          const SizedBox(height: 8),
          OutlinedButton.icon(
            onPressed: _load,
            icon: const Icon(Icons.refresh),
            label: const Text('Refresh'),
          ),
        ],
      ),
    );
  }
}
