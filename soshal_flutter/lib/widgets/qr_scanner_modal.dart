import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import '../services/permissions_service.dart';

/// Modal dialog for scanning QR codes (e.g. npub, nsec, invite link, nostr event).
///
/// On Android, displays camera feed with real-time barcode detection.
/// On desktop (or when camera is unavailable), provides a paste/manual entry fallback.
class QrScannerModal extends StatefulWidget {
  const QrScannerModal({super.key, this.title = 'Scan QR Code'});

  final String title;

  /// Convenience method to show the QR scanner modal and return the scanned result.
  static Future<String?> scan(BuildContext context,
      {String title = 'Scan QR Code'}) {
    return showModalBottomSheet<String>(
      context: context,
      isScrollControlled: true,
      backgroundColor: Colors.transparent,
      builder: (_) => QrScannerModal(title: title),
    );
  }

  @override
  State<QrScannerModal> createState() => _QrScannerModalState();
}

class _QrScannerModalState extends State<QrScannerModal> {
  MobileScannerController? _controller;
  final TextEditingController _manualText = TextEditingController();
  bool _detected = false;

  @override
  void initState() {
    super.initState();
    if (PermissionsService.isAndroid) {
      _controller = MobileScannerController(
        detectionSpeed: DetectionSpeed.normal,
        returnImage: false,
      );
    }
  }

  @override
  void dispose() {
    _controller?.dispose();
    _manualText.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_detected) return;
    for (final barcode in capture.barcodes) {
      final value = barcode.rawValue;
      if (value != null && value.isNotEmpty) {
        _detected = true;
        Navigator.of(context).pop(value);
        break;
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final isAndroid = PermissionsService.isAndroid;

    return Container(
      height: MediaQuery.of(context).size.height * 0.7,
      decoration: BoxDecoration(
        color: theme.colorScheme.surface,
        borderRadius: const BorderRadius.vertical(top: Radius.circular(20)),
      ),
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
            child: Row(
              children: [
                Text(
                  widget.title,
                  style: theme.textTheme.titleMedium?.copyWith(
                    fontWeight: FontWeight.bold,
                  ),
                ),
                const Spacer(),
                if (isAndroid && _controller != null) ...[
                  IconButton(
                    icon: const Icon(Icons.flash_on),
                    tooltip: 'Toggle Flash',
                    onPressed: () => _controller?.toggleTorch(),
                  ),
                  IconButton(
                    icon: const Icon(Icons.cameraswitch),
                    tooltip: 'Switch Camera',
                    onPressed: () => _controller?.switchCamera(),
                  ),
                ],
                IconButton(
                  icon: const Icon(Icons.close),
                  onPressed: () => Navigator.of(context).pop(),
                ),
              ],
            ),
          ),
          const Divider(height: 1),
          Expanded(
            child: isAndroid && _controller != null
                ? ClipRRect(
                    borderRadius: BorderRadius.circular(16),
                    child: Stack(
                      alignment: Alignment.center,
                      children: [
                        MobileScanner(
                          controller: _controller!,
                          onDetect: _onDetect,
                        ),
                        Container(
                          width: 240,
                          height: 240,
                          decoration: BoxDecoration(
                            border: Border.all(
                              color: theme.colorScheme.primary,
                              width: 3,
                            ),
                            borderRadius: BorderRadius.circular(16),
                          ),
                        ),
                      ],
                    ),
                  )
                : Padding(
                    padding: const EdgeInsets.all(24),
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        const Icon(
                          Icons.qr_code_scanner,
                          size: 64,
                          color: Colors.grey,
                        ),
                        const SizedBox(height: 16),
                        const Text(
                          'Camera scanning is supported on mobile devices.',
                          textAlign: TextAlign.center,
                        ),
                        const SizedBox(height: 16),
                        TextField(
                          controller: _manualText,
                          decoration: InputDecoration(
                            hintText: 'Paste or enter code/link',
                            border: const OutlineInputBorder(),
                            suffixIcon: IconButton(
                              icon: const Icon(Icons.paste),
                              onPressed: () async {
                                final data =
                                    await Clipboard.getData('text/plain');
                                if (data?.text != null) {
                                  _manualText.text = data!.text!;
                                }
                              },
                            ),
                          ),
                        ),
                        const SizedBox(height: 16),
                        ElevatedButton(
                          onPressed: () {
                            final text = _manualText.text.trim();
                            if (text.isNotEmpty) {
                              Navigator.of(context).pop(text);
                            }
                          },
                          child: const Text('Submit'),
                        ),
                      ],
                    ),
                  ),
          ),
        ],
      ),
    );
  }
}
