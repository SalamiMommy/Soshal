# Soshal Security Documentation

## Security Architecture Overview

Soshal implements a defense-in-depth security strategy across multiple layers:

- **Application Layer**: Intent validation, session management, authentication flows
- **Network Layer**: TLS configuration, certificate validation, P2P authentication
- **Cryptographic Layer**: NIP-44 encryption, key management, secure storage
- **Filesystem Layer**: Path validation, permission checks, TOCTOU protection
- **Platform Layer**: Android permissions, service isolation, secure IPC

## Security Hardening History

### 2026-09 Security Review Implementation

#### Critical Fixes
1. **Android MainActivity Export Mitigation**
   - Changed `android:exported="false"` to prevent unauthorized activity launches
   - Blocks external intent injection attacks
   - Maintains launcher functionality while securing external access

#### Medium Priority Enhancements
1. **Network Security Configuration**
   - Comprehensive TLS policy with cleartext traffic disabled by default
   - Explicit allowances for localhost and private network ranges for P2P
   - Certificate pinning framework for production endpoints
   - Development overrides for local testing

2. **QUIC Certificate Validation**
   - Hybrid certificate verifier (`HybridCertVerifier`) actively enforcing:
     system certificate validation framework + mesh-internal self-signed cert
     compatibility for P2P
   - First-contact QUIC self-signed certs accepted for mesh peers; system-cert
     (rustls-native-certs) integration still a placeholder
   - Proper TLS 1.2/1.3 signature verification framework in place

3. **Session Path Hardening**
   - Runtime TOCTOU protection for session file operations
   - Permission validation to prevent world-writable files
   - Suspicious component detection in file paths
   - Enhanced canonicalization checks

## Security Guidelines for Development

### Cryptographic Security
- Always use zeroize for sensitive key material
- Prefer established cryptographic libraries (ring, rustls)
- Implement proper key derivation (HKDF) for all derived keys
- Use constant-time comparisons for security-critical operations
- Validate all cryptographic inputs and handle errors securely

### Key Management
- Never export private key material across FFI boundaries
- Use OS keychain for persistent secret storage
- Implement proper signer lock/unlock with cache invalidation
- Validate identity-based access control for all operations
- Clear derived caches on identity switches

### Network Security
- Validate all network inputs and sanitize outputs
- Implement proper certificate validation for TLS connections
- Use HMAC-based authentication for P2P protocols
- Restrict network operations to private IP ranges where appropriate
- Implement proper timeout and connection limiting

### Input Validation
- Validate all user inputs at FFI boundaries
- Sanitize SQL queries to prevent injection
- Validate file paths to prevent directory traversal
- Implement proper length checks on all inputs
- Use allowlists rather than blocklists where possible

### Android Security
- Follow principle of least privilege for permissions
- Use exported="false" for activities unless deep linking required
- Implement proper intent validation for exported components
- Use foreground services with proper notification
- Disable unnecessary Android features (backup, etc.)

## Security Testing Requirements

### Unit Testing
- Test all security-critical functions with edge cases
- Validate error handling paths for security code
- Test cryptographic implementations with known vectors
- Verify input validation with malicious inputs

### Integration Testing
- Test security components in realistic scenarios
- Validate certificate validation with various cert types
- Test network security with different configurations
- Verify file system security with concurrent operations

### Security Testing
- Run automated security scanning tools
- Perform manual penetration testing
- Test certificate pinning bypass attempts
- Validate TOCTOU protection mechanisms

## Incident Response

### Security Incident Procedure
1. Immediately assess impact and scope
2. Contain the incident if possible
3. Preserve evidence and logs
4. Notify relevant stakeholders
5. Implement temporary mitigations
6. Root cause analysis
7. Implement permanent fixes
8. Update security documentation
9. Conduct post-incident review

### Security Bug Reporting
- Report security issues through responsible disclosure
- Provide detailed reproduction steps
- Include proof-of-concept if applicable
- Allow reasonable time for fixes before disclosure
- Coordinate with security team for public disclosure

## Compliance and Standards

### Security Standards Compliance
- Follow OWASP Mobile Security guidelines
- Implement Android security best practices
- Use industry-standard cryptographic algorithms
- Follow principle of least privilege
- Implement secure coding practices

### Regulatory Considerations
- Data protection and privacy requirements
- Export control compliance for cryptographic software
- App store security requirements
- Regional security regulations

## Future Security Roadmap

### Short-term (Next Quarter)
- Complete system certificate validation for QUIC
- Implement certificate pinning for production endpoints
- Add security audit logging
- Implement proper UID validation

### Medium-term (Next 6 Months)
- Enhanced security monitoring and alerting
- Automated security testing in CI/CD
- Additional runtime protection mechanisms
- Security-focused code review process

### Long-term (Next Year)
- Comprehensive security audit by external firm
- Enhanced threat modeling
- Security training for all developers
- Incident response simulation exercises

## Security Contact Information

- Security Team: [security@soshal.app]
- Bug Bounty: [security@soshal.app]
- PGP Key: [Available on request]

## References

- [OWASP Mobile Security](https://owasp.org/www-project-mobile-security/)
- [Android Security Best Practices](https://developer.android.com/topic/security/best-practices)
- [Rust Security Guidelines](https://doc.rust-lang.org/nomicon/security.html)
- [NIP-44 Specification](https://github.com/nostr-protocol/nips/blob/master/44.md)

---

**Last Updated**: 2026-09-21  
**Security Review Version**: 1.1  
**Next Review Date**: 2026-12-04