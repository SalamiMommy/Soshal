import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/media_service.dart';
import '../services/messaging_service.dart';
import '../services/session_service.dart';
import '../utils/media_upload.dart';

/// Edit profile: name, display name, picture URL, banner, about, NIP-05.
class EditProfileScreen extends StatefulWidget {
  /// Edit profile screen.
  const EditProfileScreen({super.key});

  @override
  State<EditProfileScreen> createState() => _EditProfileScreenState();
}

class _EditProfileScreenState extends State<EditProfileScreen> {
  final _nameController = TextEditingController();
  final _displayNameController = TextEditingController();
  final _pictureController = TextEditingController();
  final _bannerController = TextEditingController();
  final _aboutController = TextEditingController();
  final _nip05Controller = TextEditingController();
  bool _saving = false;
  bool _loaded = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _nameController.dispose();
    _displayNameController.dispose();
    _pictureController.dispose();
    _bannerController.dispose();
    _aboutController.dispose();
    _nip05Controller.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final session = context.read<SessionService>();
      final api = context.read<IdentityService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      final profile = await api.getSelfProfile(pubkey);
      _nameController.text = profile.name;
      _displayNameController.text = profile.displayName;
      _pictureController.text = profile.picture;
      _bannerController.text = profile.banner;
      _aboutController.text = profile.about;
      _nip05Controller.text = profile.nip05;
    } catch (e) {
      debugPrint('profile load: $e');
    }
    if (mounted) setState(() => _loaded = true);
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      final session = context.read<SessionService>();
      final api = context.read<IdentityService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in to edit profile');
      await api.updateProfile(
        pubkey,
        _nameController.text.trim(),
        _displayNameController.text.trim(),
        _pictureController.text.trim(),
        _bannerController.text.trim(),
        _aboutController.text.trim(),
        _nip05Controller.text.trim(),
      );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Profile saved')),
        );
        context.go('/profile');
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

  Future<void> _pickAndSet(TextEditingController controller) async {
    try {
      final blob = await pickAndUploadMedia(
        (p) => context.read<MediaService>().uploadMedia(p),
        errorMessage: 'Image upload failed (bad manifest)',
      );
      if (blob == null || !mounted) return;
      if (mounted) {
        setState(() => controller.text = blob.uri);
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Upload error: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Edit Profile'),
        actions: [
          TextButton(
            onPressed: _saving ? null : _save,
            child: const Text('Save'),
          ),
        ],
      ),
      body: !_loaded
          ? const Center(child: CircularProgressIndicator())
          : ListView(
              padding: const EdgeInsets.all(16),
              children: [
                TextField(
                  controller: _nameController,
                  decoration: const InputDecoration(labelText: 'Name'),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _displayNameController,
                  decoration: const InputDecoration(labelText: 'Display name'),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _pictureController,
                  decoration: const InputDecoration(
                    labelText: 'Picture',
                    hintText: 'pick from device or paste a URL',
                  ),
                ),
                const SizedBox(height: 8),
                OutlinedButton.icon(
                  onPressed: () => _pickAndSet(_pictureController),
                  icon: const Icon(Icons.add_photo_alternate_outlined),
                  label: const Text('Pick picture from device'),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _bannerController,
                  decoration: const InputDecoration(
                    labelText: 'Banner',
                    hintText: 'pick from device or paste a URL',
                  ),
                ),
                const SizedBox(height: 8),
                OutlinedButton.icon(
                  onPressed: () => _pickAndSet(_bannerController),
                  icon: const Icon(Icons.add_photo_alternate_outlined),
                  label: const Text('Pick banner from device'),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _aboutController,
                  maxLines: 4,
                  decoration: const InputDecoration(labelText: 'About'),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _nip05Controller,
                  keyboardType: TextInputType.url,
                  decoration: const InputDecoration(
                    labelText: 'NIP-05 identifier',
                    hintText: 'name@example.com',
                  ),
                ),
                if (_nip05Controller.text.trim().isNotEmpty)
                  Align(
                    alignment: Alignment.centerRight,
                    child: TextButton.icon(
                      onPressed: () async {
                        try {
                          final ok = await context
                              .read<IdentityService>()
                              .verifyNip05(_nip05Controller.text.trim());
                          if (context.mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(
                              SnackBar(
                                content: SelectableText(ok
                                    ? 'NIP-05 verified'
                                    : 'NIP-05 not found'),
                              ),
                            );
                          }
                        } catch (e) {
                          if (context.mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(
                              SnackBar(
                                  content: SelectableText('Verify error: $e')),
                            );
                          }
                        }
                      },
                      icon: const Icon(Icons.verified_outlined, size: 16),
                      label: const Text('Verify'),
                    ),
                  ),
                const SizedBox(height: 24),
                _saving
                    ? const Center(child: CircularProgressIndicator())
                    : const SizedBox.shrink(),
              ],
            ),
    );
  }
}
