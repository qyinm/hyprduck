/**
 * DuckDocs Landing Page JavaScript
 * Handles animations, interactions, and dynamic content
 */

(function() {
    'use strict';

    // Configuration
    const CONFIG = {
        tagline: 'Auto-capture.<br>AI-powered.<br>Documentation done.',
        typingSpeed: 60,
        typingStartDelay: 500,
        scrollThreshold: 0.15,
        mobileBreakpoint: 768
    };

    // DOM Elements
    const elements = {
        typingText: null,
        cursor: null,
        navToggle: null,
        mobileMenu: null,
        fadeElements: null
    };

    /**
     * Initialize the application
     */
    function init() {
        cacheElements();
        initTypingEffect();
        initScrollAnimations();
        initMobileMenu();
        initSmoothScroll();
    }

    /**
     * Cache DOM elements for performance
     */
    function cacheElements() {
        elements.typingText = document.querySelector('.typing-text');
        elements.cursor = document.querySelector('.cursor');
        elements.navToggle = document.querySelector('.nav-toggle');
        elements.mobileMenu = document.querySelector('.mobile-menu');
        elements.fadeElements = document.querySelectorAll('.fade-in');
    }

    /**
     * Typing effect for hero tagline
     */
    function initTypingEffect() {
        if (!elements.typingText) return;

        const text = CONFIG.tagline;
        let index = 0;
        let output = '';

        // Clear initial content
        elements.typingText.innerHTML = '';

        // Start typing after delay
        setTimeout(function typeChar() {
            if (index < text.length) {
                // Handle <br> tags as single unit
                if (text.substring(index, index + 4) === '<br>') {
                    output += '<br>';
                    index += 4;
                } else {
                    output += text.charAt(index);
                    index++;
                }
                elements.typingText.innerHTML = output;
                setTimeout(typeChar, CONFIG.typingSpeed);
            } else {
                // Hide cursor after typing completes (with delay)
                setTimeout(() => {
                    if (elements.cursor) {
                        elements.cursor.style.animation = 'none';
                        elements.cursor.style.opacity = '0';
                    }
                }, 2000);
            }
        }, CONFIG.typingStartDelay);
    }

    /**
     * Intersection Observer for scroll animations
     */
    function initScrollAnimations() {
        if (!elements.fadeElements.length) return;

        // Check for reduced motion preference
        const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

        if (prefersReducedMotion) {
            // Show all elements immediately if reduced motion is preferred
            elements.fadeElements.forEach(el => el.classList.add('visible'));
            return;
        }

        const observerOptions = {
            root: null,
            rootMargin: '0px 0px -50px 0px',
            threshold: CONFIG.scrollThreshold
        };

        const observer = new IntersectionObserver((entries) => {
            entries.forEach((entry, index) => {
                if (entry.isIntersecting) {
                    // Stagger animation for elements that appear together
                    const delay = calculateStaggerDelay(entry.target);

                    setTimeout(() => {
                        entry.target.classList.add('visible');
                    }, delay);

                    // Stop observing once animated
                    observer.unobserve(entry.target);
                }
            });
        }, observerOptions);

        elements.fadeElements.forEach(el => observer.observe(el));
    }

    /**
     * Calculate stagger delay based on element position
     */
    function calculateStaggerDelay(element) {
        // Check if element is part of a group (siblings with same class)
        const parent = element.parentElement;
        if (!parent) return 0;

        const siblings = Array.from(parent.querySelectorAll('.fade-in'));
        const index = siblings.indexOf(element);

        // Stagger siblings by 100ms each, max 400ms
        return Math.min(index * 100, 400);
    }

    /**
     * Mobile menu toggle
     */
    function initMobileMenu() {
        if (!elements.navToggle || !elements.mobileMenu) return;

        elements.navToggle.addEventListener('click', toggleMobileMenu);

        // Close menu when clicking a link
        const menuLinks = elements.mobileMenu.querySelectorAll('a');
        menuLinks.forEach(link => {
            link.addEventListener('click', closeMobileMenu);
        });

        // Close menu on escape key
        document.addEventListener('keydown', (e) => {
            if (e.key === 'Escape' && elements.mobileMenu.classList.contains('active')) {
                closeMobileMenu();
            }
        });

        // Close menu when clicking outside
        document.addEventListener('click', (e) => {
            if (!elements.mobileMenu.contains(e.target) &&
                !elements.navToggle.contains(e.target) &&
                elements.mobileMenu.classList.contains('active')) {
                closeMobileMenu();
            }
        });
    }

    function toggleMobileMenu() {
        elements.mobileMenu.classList.toggle('active');
        elements.navToggle.classList.toggle('active');

        // Update aria-expanded
        const isExpanded = elements.mobileMenu.classList.contains('active');
        elements.navToggle.setAttribute('aria-expanded', isExpanded);
    }

    function closeMobileMenu() {
        elements.mobileMenu.classList.remove('active');
        elements.navToggle.classList.remove('active');
        elements.navToggle.setAttribute('aria-expanded', 'false');
    }

    /**
     * Smooth scroll for anchor links
     */
    function initSmoothScroll() {
        document.querySelectorAll('a[href^="#"]').forEach(anchor => {
            anchor.addEventListener('click', function(e) {
                const href = this.getAttribute('href');

                // Skip if it's just "#"
                if (href === '#') return;

                const target = document.querySelector(href);
                if (!target) return;

                e.preventDefault();

                // Calculate offset for fixed header
                const headerHeight = 80;
                const targetPosition = target.getBoundingClientRect().top + window.pageYOffset - headerHeight;

                window.scrollTo({
                    top: targetPosition,
                    behavior: 'smooth'
                });

                // Close mobile menu if open
                if (elements.mobileMenu && elements.mobileMenu.classList.contains('active')) {
                    closeMobileMenu();
                }
            });
        });
    }

    /**
     * Add parallax effect to gradient orbs (subtle, performance-conscious)
     */
    function initParallax() {
        const orbs = document.querySelectorAll('.gradient-orb');
        if (!orbs.length) return;

        // Check for reduced motion preference
        const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
        if (prefersReducedMotion) return;

        let ticking = false;

        window.addEventListener('scroll', () => {
            if (!ticking) {
                window.requestAnimationFrame(() => {
                    const scrollY = window.pageYOffset;

                    orbs.forEach((orb, index) => {
                        const speed = 0.05 + (index * 0.02);
                        const yPos = scrollY * speed;
                        orb.style.transform = `translateY(${yPos}px)`;
                    });

                    ticking = false;
                });
                ticking = true;
            }
        });
    }

    /**
     * Handle nav background on scroll
     */
    function initNavScroll() {
        const nav = document.querySelector('.nav');
        if (!nav) return;

        let lastScroll = 0;

        window.addEventListener('scroll', () => {
            const currentScroll = window.pageYOffset;

            if (currentScroll > 100) {
                nav.style.background = 'rgba(10, 10, 15, 0.95)';
            } else {
                nav.style.background = 'rgba(10, 10, 15, 0.8)';
            }

            lastScroll = currentScroll;
        }, { passive: true });
    }

    // Initialize when DOM is ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }

    // Also initialize parallax and nav scroll effects
    window.addEventListener('load', () => {
        initParallax();
        initNavScroll();
    });

})();
