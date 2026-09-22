import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../services/events_service.dart';
import '../services/permissions_service.dart';

/// Standalone geohash calculator (moved out of the Network screen's
/// diagnostics tab — geohash encoding is pure math, no location service).
///
/// Primary path: type lat/lng manually and tap "Encode geohash" — works
/// offline on every platform. "Use my location" fills the fields from the
/// device position (Android GPS / Linux XDG portal) and is best-effort;
/// desktop Linux usually has no location service, so the error hint points
/// back at the manual fields. "Use IP location (approx.)" looks the
/// position up from the public IP (city-level, consent-gated) when the
/// OS location service is off.
class GeohashCalculatorScreen extends StatefulWidget {
  const GeohashCalculatorScreen({super.key});

  @override
  State<GeohashCalculatorScreen> createState() =>
      _GeohashCalculatorScreenState();
}

class _GeohashCalculatorScreenState extends State<GeohashCalculatorScreen> {
  final TextEditingController _lat = TextEditingController();
  final TextEditingController _lng = TextEditingController();
  String? _result;
  String? _error;
  bool _locating = false;

  @override
  void dispose() {
    _lat.dispose();
    _lng.dispose();
    super.dispose();
  }

  Future<void> _useMyLocation() async {
    setState(() {
      _locating = true;
      _error = null;
      _result = null;
    });
    try {
      final location = await PermissionsService.currentPosition();
      if (!mounted) return;
      if (!location.ok) {
        setState(() {
          _locating = false;
          _error =
              '${location.error}\n\nTip: enter coordinates manually below — '
              'geohash encoding needs no location service.';
        });
        return;
      }
      _lat.text = location.latitude!.toStringAsFixed(6);
      _lng.text = location.longitude!.toStringAsFixed(6);
      await _encodeGeohash(showEmpty: true);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _locating = false;
        _error = 'Location failed: $e\n\n'
            'Tip: enter coordinates manually below — geohash encoding needs '
            'no location service.';
      });
    }
  }

  /// Consent gate: IP geolocation discloses the user's public IP to a
  /// third-party provider. Never query without explicit approval.
  Future<bool> _confirmIpLocation() async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Use IP location?'),
        content: const Text(
          'Soshal will look up an approximate position from your public IP '
          'address. Your IP is sent to ipwho.is, a third-party geolocation '
          'service. The result is city-level accuracy — you can adjust the '
          'coordinates afterwards.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Allow'),
          ),
        ],
      ),
    );
    return ok ?? false;
  }

  /// Approximate fill from the egress IP — works when the OS location
  /// service is off (desktop Linux).
  Future<void> _useIpLocation() async {
    if (!await _confirmIpLocation()) return;
    if (!mounted) return;
    setState(() {
      _locating = true;
      _error = null;
      _result = null;
    });
    try {
      final location = await PermissionsService.ipLocation();
      if (!mounted) return;
      if (!location.ok) {
        setState(() {
          _locating = false;
          _error = '${location.error}\n\n'
              'Tip: enter coordinates manually below — geohash encoding '
              'needs no location service.';
        });
        return;
      }
      _lat.text = location.latitude!.toStringAsFixed(6);
      _lng.text = location.longitude!.toStringAsFixed(6);
      await _encodeGeohash(showEmpty: true);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
        content: Text('Approximate position (IP-based) — adjust if needed'),
      ));
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _locating = false;
        _error = 'IP location failed: $e\n\n'
            'Tip: enter coordinates manually below — geohash encoding needs '
            'no location service.';
      });
    }
  }

  Future<void> _encodeGeohash({bool showEmpty = false}) async {
    final lat = double.tryParse(_lat.text.trim());
    final lng = double.tryParse(_lng.text.trim());
    if (lat == null || lng == null) {
      if (showEmpty) return;
      setState(() {
        _error = 'Enter valid lat/lng numbers';
        _result = null;
      });
      return;
    }
    try {
      final geohash =
          context.read<EventsService>().encodeGeohash(lat: lat, lon: lng);
      if (!mounted) return;
      setState(() {
        _result = geohash;
        _error = null;
        _locating = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = 'Encode failed: $e';
        _result = null;
        _locating = false;
      });
    }
  }

  Future<void> _copyResult() async {
    await Clipboard.setData(ClipboardData(text: _result ?? ''));
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Geohash copied to clipboard')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      appBar: AppBar(title: const Text('Geohash Calculator')),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Geohash encoding is pure math — no location service needed. '
              'Enter coordinates or fetch them from the device.',
              style: theme.textTheme.bodySmall,
            ),
            const SizedBox(height: 16),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _lat,
                    keyboardType: const TextInputType.numberWithOptions(
                        decimal: true, signed: true),
                    decoration: const InputDecoration(
                      labelText: 'Latitude',
                      hintText: 'e.g. 51.5007',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: TextField(
                    controller: _lng,
                    keyboardType: const TextInputType.numberWithOptions(
                        decimal: true, signed: true),
                    decoration: const InputDecoration(
                      labelText: 'Longitude',
                      hintText: 'e.g. -0.1246',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                FilledButton.icon(
                  onPressed: _encodeGeohash,
                  icon: const Icon(Icons.pin_drop),
                  label: const Text('Encode geohash'),
                ),
                OutlinedButton.icon(
                  onPressed: _locating ? null : _useMyLocation,
                  icon: _locating
                      ? const SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.gps_fixed),
                  label: const Text('Use my location'),
                ),
                OutlinedButton.icon(
                  onPressed: _locating ? null : _useIpLocation,
                  icon: const Icon(Icons.language),
                  label: const Text('Use IP location (approx.)'),
                ),
              ],
            ),
            if (_result != null) ...[
              const SizedBox(height: 16),
              Card(
                child: ListTile(
                  title: const Text('Geohash (9 chars)'),
                  subtitle: SelectableText(
                    _result!,
                    style: const TextStyle(fontFamily: 'monospace'),
                  ),
                  trailing: IconButton(
                    icon: const Icon(Icons.copy),
                    tooltip: 'Copy',
                    onPressed: _copyResult,
                  ),
                ),
              ),
            ],
            if (_error != null)
              Padding(
                padding: const EdgeInsets.only(top: 16),
                child: Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: theme.colorScheme.errorContainer,
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: SelectableText(
                    _error!,
                    style: TextStyle(color: theme.colorScheme.onErrorContainer),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
