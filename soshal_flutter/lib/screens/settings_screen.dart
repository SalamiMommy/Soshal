import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';
import '../services/network_service.dart';
import 'share_app_screen.dart';

/// Settings Screen
/// Relay config, privacy, backup
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late List<String> _relays;

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
                title: const Text('Edit Profile'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/edit-profile'),
              ),
              ListTile(
                title: const Text('Accounts'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/accounts'),
              ),
              ListTile(
                title: const Text('Backup'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/backup'),
              ),
              ListTile(
                title: const Text('Turso Database Sync'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/turso'),
              ),
              ListTile(
                title: const Text('Share Soshal'),
                leading: const Icon(Icons.share),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => Navigator.of(context).push(
                  MaterialPageRoute(builder: (_) => const ShareAppScreen()),
                ),
              ),
            ]),
            // Privacy section
            _buildSection('Privacy & Security', [
              ListTile(
                title: const Text('Privacy'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/privacy'),
              ),
              ListTile(
                title: const Text('Privacy Level'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => _showPrivacyDialog(),
              ),
              ListTile(
                title: const Text('Blocked Users'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/blocked'),
              ),
              ListTile(
                title: const Text('Moderation'),
                subtitle: const Text('Mutes, blocks, word filters'),
                leading: const Icon(Icons.gpp_maybe_outlined),
                onTap: () => context.go('/settings/moderation'),
              ),
              ListTile(
                title: const Text('Session Security'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/security'),
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
            // Discover section
            _buildSection('Discover', [
              ListTile(
                title: const Text('Dating'),
                subtitle: const Text('Cards, likes, matches'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/dating'),
              ),
              ListTile(
                title: const Text('Events'),
                subtitle: const Text('Nearby meetups, RSVP, check-in'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/events'),
              ),
              ListTile(
                title: const Text('Groups'),
                subtitle: const Text('NIP-29 communities'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/groups'),
              ),
              ListTile(
                title: const Text('Marketplace'),
                subtitle: const Text('Listings, orders, escrow'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/marketplace'),
              ),
              ListTile(
                title: const Text('Live'),
                subtitle: const Text('Streams, presence'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/live'),
              ),
              ListTile(
                title: const Text('Stories'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/stories'),
              ),
              ListTile(
                title: const Text('Search'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/search'),
              ),
            ]),
            // Appearance section
            _buildSection('Appearance', [
              ListTile(
                title: const Text('Appearance'),
                subtitle: const Text('Theme, accent, font size'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/appearance'),
              ),
              ListTile(
                title: const Text('Language'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/language'),
              ),
            ]),
            // Network section
            _buildSection('Network', [
              ListTile(
                title: const Text('Network Settings'),
                subtitle: const Text('Relays, transports'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/network'),
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
                onTap: () => context.go('/settings/notifications'),
              ),
            ]),
            // About section
            _buildSection('About', [
              ListTile(
                title: const Text('Storage'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/storage'),
              ),
              ListTile(
                title: const Text('Advanced'),
                subtitle: const Text('Transports, diagnostics'),
                trailing: const Icon(Icons.arrow_forward),
                onTap: () => context.go('/settings/advanced'),
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

  void _showPrivacyDialog() {
    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Privacy Level'),
          content: const Text('Choose your privacy level'),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Public'),
            ),
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Friends Only'),
            ),
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Private'),
            ),
          ],
        );
      },
    );
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
                  onPressed: () {
                    setState(() => _relays.removeAt(index));
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

  void _showLogoutDialog() {
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
                context.go('/auth');
              },
              child: const Text('Logout'),
            ),
          ],
        );
      },
    );
  }
}
