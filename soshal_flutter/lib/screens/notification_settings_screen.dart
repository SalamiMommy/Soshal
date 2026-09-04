import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/notifications_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../widgets/settings_scaffold.dart';

/// Notification settings: push registration status + in-app toggles.
class NotificationSettingsScreen extends StatefulWidget {
  /// Notification settings screen.
  const NotificationSettingsScreen({super.key});

  @override
  State<NotificationSettingsScreen> createState() =>
      _NotificationSettingsScreenState();
}

class _NotificationSettingsScreenState
    extends State<NotificationSettingsScreen> {
  bool _pushEnabled = false;
  bool _loaded = false;
  String _token = '';

  @override
  void initState() {
    super.initState();
    _loadPushState();
  }

  Future<void> _loadPushState() async {
    try {
      final json = await context.read<SessionService>().getActiveAccountJson();
      final token = json['push_token'] as String?;
      if (mounted) {
        setState(() {
          _token = token ?? '';
          _pushEnabled = _token.isNotEmpty;
          _loaded = true;
        });
      }
    } catch (e) {
      debugPrint('notification settings: load push state: $e');
      if (mounted) setState(() => _loaded = true);
    }
  }

  Future<void> _togglePush(bool enabled) async {
    final session = context.read<SessionService>();
    final api = context.read<NotificationService>();
    final pubkey = session.activePubkey;
    if (pubkey == null) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: SelectableText('Sign in to change push settings')),
      );
      return;
    }
    if (!enabled) {
      try {
        await api.unregisterPush(pubkey);
        if (!mounted) return;
        setState(() => _pushEnabled = false);
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
              content: Text('Push notifications disabled for this device.')),
        );
      } catch (e) {
        debugPrint('notification settings: disable push: $e');
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Failed to disable push: $e')),
          );
        }
      }
      return;
    }
    // Enabling requires an FCM device token, which is not minted in this
    // build (needs google-services.json). If one is already on record,
    // register it; otherwise stay honest: no fake toggle.
    if (_token.isNotEmpty) {
      try {
        // Mirror the token into the Rust session file for the active
        // account; the settings screen reads it back on load.
        await session.registerPushToken(_token);
        await api.registerPush(pubkey, _token);
        if (!mounted) return;
        setState(() => _pushEnabled = true);
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
              content: Text('Push notifications enabled for this device.')),
        );
      } catch (e) {
        debugPrint('notification settings: enable push: $e');
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Failed to enable push: $e')),
          );
        }
      }
      return;
    }
    if (!mounted) return;
    showDialog<void>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Push notifications unavailable'),
        content: const Text(
          'Enabling push needs Firebase Cloud Messaging setup '
          '(google-services.json) in the Android build. A device token '
          'cannot be minted yet, so push stays off for this device.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(),
            child: const Text('OK'),
          ),
        ],
      ),
    );
  }

  String _tokenLabel() {
    if (!_loaded) return 'Checking device token status…';
    if (_token.isEmpty) {
      return 'No device token registered. Requires FCM setup '
          '(google-services.json) in the Android build.';
    }
    final masked = shortPubkey(_token, head: 6, tail: 4);
    return 'Registered token: $masked';
  }

  @override
  Widget build(BuildContext context) {
    return SettingsScaffold(
      title: 'Notifications',
      children: [
        SwitchListTile(
          title: const Text('In-app notifications'),
          subtitle: const Text('Notifications tab, badges, and unread counts'),
          value: true,
          onChanged: (_) {
            ScaffoldMessenger.of(context).showSnackBar(
              const SnackBar(
                content: SelectableText('In-app notifications are always on'),
              ),
            );
          },
        ),
        SwitchListTile(
          title: const Text('Push notifications'),
          subtitle: Text(
            'Delivered by FCM when the app is backgrounded. '
            '${_tokenLabel()}',
          ),
          value: _pushEnabled,
          onChanged: _togglePush,
        ),
        const ListTile(
          leading: Icon(Icons.info_outline),
          title: Text('About'),
          subtitle:
              Text('Push tokens are registered per account and stored in the '
                  'session file.'),
        ),
      ],
    );
  }
}
