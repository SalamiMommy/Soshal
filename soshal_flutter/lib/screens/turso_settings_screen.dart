import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/turso_service.dart';
import '../widgets/error_state_text.dart';

/// Turso Database Sync Settings Screen
class TursoSettingsScreen extends StatefulWidget {
  const TursoSettingsScreen({super.key});

  @override
  State<TursoSettingsScreen> createState() => _TursoSettingsScreenState();
}

class _TursoSettingsScreenState extends State<TursoSettingsScreen> {
  final _formKey = GlobalKey<FormState>();
  final _urlController = TextEditingController();
  final _tokenController = TextEditingController();

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        context.read<TursoService>().checkStatus();
      }
    });
  }

  @override
  void dispose() {
    _urlController.dispose();
    _tokenController.dispose();
    super.dispose();
  }

  void _saveCredentials() async {
    if (_formKey.currentState!.validate()) {
      final tursoService = context.read<TursoService>();
      final success = await tursoService.configure(
        url: _urlController.text.trim(),
        authToken: _tokenController.text.trim(),
      );

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              success
                  ? 'Turso connection configured successfully'
                  : 'Failed to configure Turso connection',
            ),
          ),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Turso Database Sync'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            tooltip: 'Refresh status',
            onPressed: () => context.read<TursoService>().checkStatus(),
          ),
        ],
      ),
      body: Consumer<TursoService>(
        builder: (context, turso, child) {
          return SingleChildScrollView(
            padding: const EdgeInsets.all(16.0),
            child: Form(
              key: _formKey,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Card(
                    child: Padding(
                      padding: const EdgeInsets.all(16.0),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Row(
                            children: [
                              Icon(Icons.cloud_sync,
                                  color:
                                      Theme.of(context).colorScheme.primary),
                              const SizedBox(width: 8),
                              const Text(
                                'Turso Edge Replication',
                                style: TextStyle(
                                  fontSize: 18,
                                  fontWeight: FontWeight.bold,
                                ),
                              ),
                            ],
                          ),
                          SizedBox(height: 8),
                          Text(
                            'Connect your local embedded database to Turso Cloud for seamless multi-device replication and cloud backups.',
                            style: TextStyle(color: Colors.grey),
                          ),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(height: 24),
                  Text(
                    'Status',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 8),
                  ListTile(
                    tileColor: Theme.of(context).cardColor,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(8),
                    ),
                    leading: Icon(
                      turso.isConfigured ? Icons.check_circle : Icons.warning,
                      color: turso.isConfigured ? Colors.green : Colors.amber,
                    ),
                    title: Text(
                        turso.isConfigured ? 'Configured' : 'Not Configured'),
                    subtitle: Text('Status: ${turso.status.toUpperCase()}'),
                    trailing: turso.isSyncing
                        ? const SizedBox(
                            width: 20,
                            height: 20,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : TextButton.icon(
                            onPressed: turso.isConfigured
                                ? () => turso.syncNow()
                                : null,
                            icon: const Icon(Icons.sync),
                            label: const Text('Sync Now'),
                          ),
                  ),
                  if (turso.lastError != null) ...[
                    const SizedBox(height: 12),
                    ErrorStateText('Error: ${turso.lastError}'),
                  ],
                  const SizedBox(height: 24),
                  Text(
                    'Credentials',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 12),
                  TextFormField(
                    controller: _urlController,
                    decoration: const InputDecoration(
                      labelText: 'Database URL',
                      hintText: 'libsql://your-db.turso.io',
                      border: OutlineInputBorder(),
                    ),
                    validator: (v) {
                      if (v == null || v.trim().isEmpty) {
                        return 'Please enter database URL';
                      }
                      return null;
                    },
                  ),
                  const SizedBox(height: 16),
                  TextFormField(
                    controller: _tokenController,
                    obscureText: true,
                    decoration: const InputDecoration(
                      labelText: 'Auth Token',
                      border: OutlineInputBorder(),
                    ),
                    validator: (v) {
                      if (v == null || v.trim().isEmpty) {
                        return 'Please enter auth token';
                      }
                      return null;
                    },
                  ),
                  const SizedBox(height: 24),
                  SizedBox(
                    width: double.infinity,
                    height: 48,
                    child: ElevatedButton.icon(
                      onPressed: _saveCredentials,
                      icon: const Icon(Icons.save),
                      label: const Text('Save & Connect'),
                    ),
                  ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}
