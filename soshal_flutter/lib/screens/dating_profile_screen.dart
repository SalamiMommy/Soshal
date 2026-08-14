import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/dating_service.dart';
import '../services/session_service.dart';

/// My dating profile: create or edit.
class DatingProfileScreen extends StatefulWidget {
  /// My dating profile screen.
  const DatingProfileScreen({super.key});

  @override
  State<DatingProfileScreen> createState() => _DatingProfileScreenState();
}

class _DatingProfileScreenState extends State<DatingProfileScreen> {
  final _name = TextEditingController();
  final _age = TextEditingController();
  final _location = TextEditingController();
  final _bio = TextEditingController();
  final _interests = TextEditingController();
  final _images = TextEditingController();
  bool _saving = false;
  bool _hasProfile = false;

  @override
  void initState() {
    super.initState();
    _load();
    _stats = _loadStats();
  }

  @override
  void dispose() {
    _name.dispose();
    _age.dispose();
    _location.dispose();
    _bio.dispose();
    _interests.dispose();
    _images.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final session = context.read<SessionService>();
      final api = context.read<DatingService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      try {
        final own = await api.getOwnProfile(pubkey);
        _name.text = own.name;
        _age.text = own.age > 0 ? '${own.age}' : '';
        _location.text = own.location;
        _bio.text = own.bio;
        _interests.text = own.interests.join(', ');
        _images.text = own.images.join(', ');
        _hasProfile = true;
      } catch (_) {
        _hasProfile = false;
      }
    } catch (e) {
      debugPrint('dating me load: $e');
    }
    if (mounted) setState(() {});
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      final session = context.read<SessionService>();
      final api = context.read<DatingService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in');
      final images = _images.text
          .split(',')
          .map((e) => e.trim())
          .where((e) => e.isNotEmpty)
          .toList();
      final interests = _interests.text
          .split(',')
          .map((e) => e.trim())
          .where((e) => e.isNotEmpty)
          .toList();
      if (_hasProfile) {
        await api.updateProfile(pubkey, _bio.text.trim(), images, interests);
      } else {
        await api.createProfile(
          pubkey,
          _name.text.trim(),
          int.tryParse(_age.text.trim()) ?? 0,
          _location.text.trim(),
          _bio.text.trim(),
          images,
          interests,
        );
      }
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Dating profile saved')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Save error: $e')));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Widget _stat(String label, String value) {
    return Column(
      children: [
        Text(value,
            style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 16)),
        Text(label, style: const TextStyle(fontSize: 11)),
      ],
    );
  }

  Future<void> _delete() async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      await context.read<DatingService>().deleteProfile(pubkey);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Dating profile deleted')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Delete error: $e')));
      }
    }
  }

  late Future<DatingStats?> _stats;

  Future<DatingStats?> _loadStats() async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return null;
      return await context.read<DatingService>().getStats(pubkey);
    } catch (_) {
      return null;
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title:
            Text(_hasProfile ? 'Edit dating profile' : 'Create dating profile'),
        actions: [
          TextButton(
            onPressed: _saving ? null : _save,
            child: const Text('Save'),
          ),
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          FutureBuilder<DatingStats?>(
            future: _stats,
            builder: (context, snapshot) {
              final st = snapshot.data;
              if (st == null) return const SizedBox.shrink();
              return Card(
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.spaceAround,
                    children: [
                      _stat('Matches', '${st.matches}'),
                      _stat('Likes', '${st.likesReceived}'),
                      _stat('Views', '${st.profileViews}'),
                      _stat('Photos', '${st.photoCount}'),
                    ],
                  ),
                ),
              );
            },
          ),
          const SizedBox(height: 12),
          if (_hasProfile) ...[
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.badge_outlined),
              title: Text(_name.text.trim()),
              dense: true,
            ),
          ],
          TextField(
            controller: _name,
            enabled: !_hasProfile,
            decoration: const InputDecoration(labelText: 'Name'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _age,
            enabled: !_hasProfile,
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(labelText: 'Age'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _location,
            enabled: !_hasProfile,
            decoration: const InputDecoration(labelText: 'Location'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _bio,
            maxLines: 4,
            decoration: const InputDecoration(labelText: 'Bio'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _interests,
            decoration: const InputDecoration(
              labelText: 'Interests',
              hintText: 'comma separated: hiking, music, books',
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _images,
            decoration: const InputDecoration(
              labelText: 'Images',
              hintText: 'comma separated image URLs',
            ),
          ),
          const SizedBox(height: 24),
          _saving
              ? const Center(child: CircularProgressIndicator())
              : const SizedBox.shrink(),
          if (_hasProfile) ...[
            const SizedBox(height: 24),
            OutlinedButton.icon(
              onPressed: _delete,
              icon: const Icon(Icons.delete_outline),
              label: const Text('Delete dating profile'),
              style: OutlinedButton.styleFrom(
                foregroundColor: Colors.red,
              ),
            ),
          ],
        ],
      ),
    );
  }
}
