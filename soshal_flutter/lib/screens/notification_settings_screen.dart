import 'package:flutter/material.dart';

/// Notification settings: push registration status + in-app toggles.
class NotificationSettingsScreen extends StatelessWidget {
  /// Notification settings screen.
  const NotificationSettingsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Notifications')),
      body: ListView(
        children: [
          SwitchListTile(
            title: const Text('In-app notifications'),
            subtitle:
                const Text('Notifications tab, badges, and unread counts'),
            value: true,
            onChanged: (_) {
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(
                  content: SelectableText('In-app notifications are always on'),
                ),
              );
            },
          ),
          const SwitchListTile(
            title: Text('Push notifications'),
            subtitle:
                Text('Delivered by FCM when the app is backgrounded. Requires '
                    'google-services.json in the Android build.'),
            value: true,
            onChanged: null,
          ),
          const ListTile(
            leading: Icon(Icons.info_outline),
            title: Text('About'),
            subtitle:
                Text('Push tokens are registered per account and stored in the '
                    'session file. Disable push for this device by removing '
                    'the token in a future release.'),
          ),
        ],
      ),
    );
  }
}
