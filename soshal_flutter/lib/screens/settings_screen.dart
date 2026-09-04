import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'dart:convert';
import '../services/session_service.dart';
import '../services/settings_service.dart';
import '../services/network_service.dart';
import '../services/zap_service.dart';
import '../utils/format.dart';
import '../utils/dialog_guard.dart';

/// Settings Screen
/// Relay config, privacy, backup
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late List<String> _relays;
  bool _autologinEnabled = true;

  @override
  void initState() {
    super.initState();
    _loadSettings();
  }

  void _loadSettings() {
    final sessionService = context.read<SessionService>();
    final activeAccount = sessionService.activeAccount;
    if (activeAccount != null) {
      _relays = List.from(activeAccount.relayList);
    } else {
      _relays = [];
    }
    final settings = context.read<SettingsService>();
    _autologinEnabled = settings.getSetting('autologin_enabled') != 'false';
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: SingleChildScrollView(
        child: Column(
          children: [
            // Account section
            _buildSection('Account', [
              ListTile(
                title: const Text('Backup'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/backup'),
              ),
              ListTile(
                title: const Text('Turso Database Sync'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/turso'),
              ),
              ListTile(
                title: const Text('Share Soshal'),
                leading: const Icon(Icons.share),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/share'),
              ),
            ]),
            // Privacy section
            _buildSection('Privacy & Security', [
              ListTile(
                title: const Text('Blocked Users'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/blocked'),
              ),
              ListTile(
                title: const Text('Moderation'),
                subtitle: const Text('Mutes, blocks, word filters'),
                leading: const Icon(Icons.gpp_maybe_outlined),
                onTap: () => context.push('/settings/moderation'),
              ),
              ListTile(
                title: const Text('Session Security'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/security'),
              ),
              SwitchListTile(
                title: const Text('Autologin'),
                subtitle: const Text(
                    'Sign in to the last account used automatically on launch'),
                value: _autologinEnabled,
                onChanged: (value) async {
                  final settings = context.read<SettingsService>();
                  await settings.setSetting(
                      'autologin_enabled', value.toString());
                  setState(() => _autologinEnabled = value);
                },
              ),
            ]),
            // Relay section
            _buildSection('Relays', [
              ListTile(
                title: const Text('Relay Configuration'),
                subtitle: Text('${_relays.length} relay(s)'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => _showRelayDialog(),
              ),
              ..._relays.map((relay) {
                return ListTile(
                  contentPadding: EdgeInsets.only(left: 32, right: 16),
                  title: Text(relay),
                  trailing: IconButton(
                    icon: const Icon(Icons.delete),
                    onPressed: () async {
                      setState(() => _relays.remove(relay));
                      final session = context.read<SessionService>();
                      final pubkey = session.activePubkey;
                      if (pubkey != null) {
                        await session.updateRelays(pubkey, _relays);
                        _publishRelayList();
                      }
                    },
                  ),
                );
              }),
              ListTile(
                contentPadding: const EdgeInsets.only(left: 32, right: 16),
                title: const Text('Add Relay'),
                trailing: const Icon(Icons.add),
                onTap: () => _showAddRelayDialog(),
              ),
            ]),
            // Appearance section
            _buildSection('Appearance', [
              ListTile(
                title: const Text('Appearance'),
                subtitle: const Text('Theme, accent, font size'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/appearance'),
              ),
              ListTile(
                title: const Text('Language'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/language'),
              ),
            ]),
            // Lightning section
            _buildSection('Lightning', [
              Consumer<ZapService>(
                builder: (context, zap, _) => ListTile(
                  title: const Text('NWC Wallet'),
                  subtitle: Text(
                    zap.isConnected
                        ? 'Connected · ${zap.nwcPubkey != null ? prefixEllipsis(zap.nwcPubkey!, 16) : ''}'
                        : 'Not connected',
                  ),
                  trailing: IconButton(
                    icon: Icon(
                      zap.isConnected ? Icons.link_off : Icons.add_link,
                    ),
                    onPressed: () => _showNwcDialog(zap),
                  ),
                ),
              ),
              ListTile(
                title: const Text('Resolve LNURL'),
                subtitle: const Text('Parse a lud16 address'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => _showLnurlDialog(),
              ),
            ]),
            // Network section
            _buildSection('Network', [
              ListTile(
                title: const Text('Network Settings'),
                subtitle: const Text('Relays, transports'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/network'),
              ),
              Consumer<NetworkService>(
                builder: (context, net, _) => ListTile(
                  title: const Text('Transports'),
                  subtitle: Text(
                    'I2P: ${net.i2p == null ? 'unknown' : net.i2p! ? 'up' : 'down'} · '
                    'Freenet: ${net.freenet == null ? 'unknown' : net.freenet! ? 'up' : 'down'}',
                  ),
                  trailing: IconButton(
                    icon: const Icon(Icons.refresh),
                    onPressed: () => net.refresh(),
                  ),
                ),
              ),
            ]),
            // Notifications section
            _buildSection('Notifications', [
              ListTile(
                title: const Text('Push Notifications'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/notifications'),
              ),
            ]),
            // About section
            _buildSection('About', [
              ListTile(
                title: const Text('Storage'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/storage'),
              ),
              ListTile(
                title: const Text('Advanced'),
                subtitle: const Text('Transports, diagnostics'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.push('/settings/advanced'),
              ),
              const ListTile(
                title: Text('App Version'),
                subtitle: Text('0.1.0'),
              ),
              ListTile(
                title: const Text('Logout'),
                onTap: () => _showLogoutDialog(),
              ),
            ]),
          ],
        ),
      ),
    );
  }

  Widget _buildSection(String title, List<Widget> children) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 24, 16, 8),
          child: Text(
            title,
            style: const TextStyle(
              fontSize: 14,
              fontWeight: FontWeight.bold,
              color: Colors.grey,
            ),
          ),
        ),
        ...children,
        const Divider(),
      ],
    );
  }

  /// Publishes the current relay list as a kind-10002 relay-list event via
  /// the bridge (best-effort; failures surface as a snackbar).
  void _publishRelayList() {
    try {
      context.read<NetworkService>().publishRelayList(relayUrls: _relays);
    } catch (e) {
      debugPrint('publish relay list: $e');
      ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Publish relay list: $e')));
    }
  }

  void _showRelayDialog() {
    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Relay Configuration'),
          content: ListView.builder(
            itemCount: _relays.length,
            itemBuilder: (context, index) {
              return ListTile(
                title: Text(_relays[index]),
                trailing: IconButton(
                  icon: const Icon(Icons.delete),
                  onPressed: () async {
                    setState(() => _relays.removeAt(index));
                    final session = context.read<SessionService>();
                    final pubkey = session.activePubkey;
                    if (pubkey != null) {
                      await session.updateRelays(pubkey, _relays);
                    }
                    _publishRelayList();
                    if (!context.mounted) return;
                    Navigator.of(context).pop();
                  },
                ),
              );
            },
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Close'),
            ),
          ],
        );
      },
    );
  }

  void _showAddRelayDialog() {
    final controller = TextEditingController();
    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Add Relay'),
          content: TextField(
            controller: controller,
            decoration: const InputDecoration(
              hintText: 'wss://relay.example.com',
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () async {
                if (controller.text.isNotEmpty) {
                  setState(() => _relays.add(controller.text));
                  final session = context.read<SessionService>();
                  final pubkey = session.activePubkey;
                  if (pubkey != null) {
                    await session.updateRelays(pubkey, _relays);
                  }
                  _publishRelayList();
                  if (!context.mounted) return;
                  Navigator.of(context).pop();
                }
              },
              child: const Text('Add'),
            ),
          ],
        );
      },
    );
  }

  void _showNwcDialog(ZapService zap) {
    if (zap.isConnected) {
      showDialog(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('Disconnect NWC wallet?'),
          content:
              const Text('Disconnects the Nostr Wallet Connect wallet from zap '
                  'payments. The connection URI is discarded.'),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () async {
                Navigator.of(context).pop();
                try {
                  await zap.disconnect();
                  if (!context.mounted) return;
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(
                        content: SelectableText('NWC wallet disconnected')),
                  );
                } catch (e) {
                  if (!context.mounted) return;
                  ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                      content: SelectableText('Disconnect failed: $e')));
                }
              },
              child: const Text('Disconnect'),
            ),
          ],
        ),
      );
      return;
    }
    final controller = TextEditingController();
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Connect NWC wallet'),
        content: TextField(
          controller: controller,
          decoration: const InputDecoration(
            labelText: 'nostr+walletconnect:// URI',
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () async {
              final uri = controller.text.trim();
              if (uri.isEmpty) return;
              try {
                await zap.connect(uri);
              } catch (e) {
                if (!context.mounted) return;
                Navigator.of(context).pop();
                ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: SelectableText('Connect failed: $e')));
                return;
              }
              if (!context.mounted) return;
              Navigator.of(context).pop();
            },
            child: const Text('Connect'),
          ),
        ],
      ),
    );
  }

  void _showLnurlDialog() {
    final controller = TextEditingController();
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Resolve LNURL'),
        content: TextField(
          controller: controller,
          decoration: const InputDecoration(
            labelText: 'LN address (lud16)',
            hintText: 'name@domain.com',
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () async {
              final lnurl = controller.text.trim();
              if (lnurl.isEmpty) return;
              try {
                final json = await context.read<ZapService>().parseLnurl(lnurl);
                if (!context.mounted) return;
                Navigator.of(context).pop();
                final meta = jsonDecode(json) as Map<String, dynamic>;
                showDialogDeferred(
                  context: context,
                  builder: (context) => AlertDialog(
                    title: const Text('LNURL resolved'),
                    content: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Name: ${meta['name']}'),
                        Text('Domain: ${meta['domain']}'),
                        Text('Callback: ${meta['callback']}'),
                      ],
                    ),
                    actions: [
                      TextButton(
                        onPressed: () => Navigator.of(context).pop(),
                        child: const Text('Close'),
                      ),
                    ],
                  ),
                );
              } catch (e) {
                if (!context.mounted) return;
                Navigator.of(context).pop();
                ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: SelectableText('Parse failed: $e')));
              }
            },
            child: const Text('Resolve'),
          ),
        ],
      ),
    );
  }

  void _showLogoutDialog() {
    final screenContext = context;
    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Logout'),
          content: const Text('Are you sure you want to logout?'),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () {
                Navigator.of(context).pop();
                WidgetsBinding.instance.addPostFrameCallback((_) {
                  if (screenContext.mounted) screenContext.go('/auth');
                });
              },
              child: const Text('Logout'),
            ),
          ],
        );
      },
    );
  }
}
